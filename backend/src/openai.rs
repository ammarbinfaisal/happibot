use anyhow::Context;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

fn api_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}

// ── Whisper transcription ──

pub async fn transcribe(file_bytes: Vec<u8>, filename: &str) -> anyhow::Result<String> {
    let api_key = api_key().ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

    let part = multipart::Part::bytes(file_bytes)
        .file_name(filename.to_string())
        .mime_str("audio/ogg")?;

    let form = multipart::Form::new()
        .text("model", "whisper-1")
        .text("response_format", "text")
        .part("file", part);

    let resp = reqwest::Client::new()
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(&api_key)
        .multipart(form)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Whisper API {status}: {body}");
    }

    Ok(resp.text().await?.trim().to_string())
}

// ── Responses API for GPT-5.4-pro ──

#[derive(Debug, Serialize)]
struct ResponseRequest {
    model: String,
    instructions: String,
    input: Vec<ResponseInputMessage>,
    reasoning: ResponseReasoning,
    text: ResponseTextConfig,
}

#[derive(Debug, Serialize)]
struct ResponseReasoning {
    effort: String,
}

#[derive(Debug, Serialize)]
struct ResponseTextConfig {
    verbosity: String,
    format: Value,
}

#[derive(Debug, Serialize)]
struct ResponseInputMessage {
    role: String,
    content: Vec<ResponseInputContent>,
}

#[derive(Debug, Serialize)]
struct ResponseInputContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct ResponseApiResponse {
    #[serde(default)]
    output: Vec<ResponseOutputItem>,
}

#[derive(Debug, Deserialize)]
struct ResponseOutputItem {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    content: Vec<ResponseOutputContent>,
}

#[derive(Debug, Deserialize)]
struct ResponseOutputContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

fn input_text_message(role: impl Into<String>, text: impl Into<String>) -> ResponseInputMessage {
    ResponseInputMessage {
        role: role.into(),
        content: vec![ResponseInputContent {
            content_type: "input_text".to_string(),
            text: text.into(),
        }],
    }
}

fn normalize_history_role(role: &str) -> &str {
    match role {
        "assistant" => "assistant",
        _ => "user",
    }
}

fn json_schema_format(name: &str, schema: Value) -> Value {
    json!({
        "type": "json_schema",
        "name": name,
        "schema": schema,
        "strict": true,
    })
}

fn extract_output_text(resp: &ResponseApiResponse) -> Option<&str> {
    resp.output
        .iter()
        .filter(|item| item.item_type == "message")
        .flat_map(|item| item.content.iter())
        .find_map(|content| {
            (content.content_type == "output_text")
                .then_some(content.text.as_deref())
                .flatten()
        })
}

async fn create_structured_response(
    model: String,
    instructions: String,
    input: Vec<ResponseInputMessage>,
    reasoning_effort: String,
    verbosity: String,
    schema_name: &str,
    schema: Value,
) -> anyhow::Result<String> {
    let api_key = api_key().ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

    let req = ResponseRequest {
        model,
        instructions,
        input,
        reasoning: ResponseReasoning {
            effort: reasoning_effort,
        },
        text: ResponseTextConfig {
            verbosity,
            format: json_schema_format(schema_name, schema),
        },
    };

    let resp = reqwest::Client::new()
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(&api_key)
        .json(&req)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI responses API {status}: {body}");
    }

    let response: ResponseApiResponse = resp
        .json()
        .await
        .context("failed to decode OpenAI responses payload")?;

    extract_output_text(&response)
        .map(str::to_string)
        .context("OpenAI response did not include assistant output_text")
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "intent")]
pub enum ParsedIntent {
    #[serde(rename = "mood")]
    Mood {
        happiness: i64,
        energy: i64,
        stress: i64,
        note: Option<String>,
    },
    #[serde(rename = "progress")]
    Progress {
        goal_title: String,
        value: Option<f64>,
        note: Option<String>,
    },
    #[serde(rename = "create_goal")]
    CreateGoal { title: String, why: Option<String> },
    #[serde(rename = "chat")]
    Chat { reply: String },
}

#[derive(Debug, Deserialize)]
struct StructuredIntent {
    intent: String,
    #[serde(default)]
    happiness: Option<i64>,
    #[serde(default)]
    energy: Option<i64>,
    #[serde(default)]
    stress: Option<i64>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    goal_title: Option<String>,
    #[serde(default)]
    value: Option<f64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    reply: Option<String>,
}

impl TryFrom<StructuredIntent> for ParsedIntent {
    type Error = anyhow::Error;

    fn try_from(value: StructuredIntent) -> Result<Self, Self::Error> {
        match value.intent.as_str() {
            "mood" => Ok(Self::Mood {
                happiness: value
                    .happiness
                    .context("structured intent missing `happiness` for mood")?,
                energy: value
                    .energy
                    .context("structured intent missing `energy` for mood")?,
                stress: value
                    .stress
                    .context("structured intent missing `stress` for mood")?,
                note: value.note,
            }),
            "progress" => Ok(Self::Progress {
                goal_title: value
                    .goal_title
                    .context("structured intent missing `goal_title` for progress")?,
                value: value.value,
                note: value.note,
            }),
            "create_goal" => Ok(Self::CreateGoal {
                title: value
                    .title
                    .context("structured intent missing `title` for create_goal")?,
                why: value.why,
            }),
            "chat" => Ok(Self::Chat {
                reply: value
                    .reply
                    .context("structured intent missing `reply` for chat")?,
            }),
            other => anyhow::bail!("unknown structured intent `{other}`"),
        }
    }
}

const FALLBACK_REPLY: &str = "I'm having trouble thinking right now. Try again?";

const SYSTEM_PROMPT: &str = r#"<role>
You are Happi, a Telegram wellbeing coach focused on helping the user become happier by making progress on meaningful goals.
</role>

<mission>
Increase the user's happiness by helping them choose, pursue, and sustain meaningful goals. Treat progress toward meaningful goals as a primary driver of happiness, but never at the expense of empathy, realism, or trust.
</mission>

<default_follow_through_policy>
- If the user's intent is clear and you have enough information for a reversible action, complete it.
- Ask one short clarifying question only when required information is missing or genuinely ambiguous.
- Do not ask unnecessary follow-up questions when a reasonable inference is available from the supplied context.
</default_follow_through_policy>

<decision_policy>
Choose exactly one intent:
1. mood: the user is reporting happiness, energy, or stress specifically enough to log.
2. progress: the user is updating progress on a known goal or a clearly implied goal.
3. create_goal: the user is defining a new goal or clearly asking to set one.
4. chat: everything else, including messages that need support, coaching, or clarification.

When deciding between logging and coaching:
- Prioritize emotional attunement when the user seems stressed, ashamed, discouraged, overwhelmed, or vulnerable.
- Prioritize progress reinforcement when the user made progress, even if small.
- If the user sounds stuck, lower the bar and guide them toward the smallest meaningful next step.
- If the user has no clear goal but wants change, help them define a goal that is concrete and personally meaningful.
- Do not force tracking when the user mainly needs reflection, encouragement, or a clarifying question.
</decision_policy>

<conversation_rules>
- Continue threads naturally when the user's message answers a prior question.
- If the user ignores a prior question, pivot gracefully without insisting.
- Use active goals, observations, and retrieved context only when they materially improve relevance.
- If you reference prior context, do it naturally and cautiously.
- Never invent facts, memories, or goal progress that are not supported by the supplied context.
</conversation_rules>

<reply_quality_bar>
- Be warm, concise, and specific.
- The reply should usually be 1-3 sentences.
- Match the user's energy.
- Prefer one concrete next step over generic motivation.
- Use at most one emoji, and only if it adds warmth.
- Avoid sounding clinical, preachy, or interrogative.
</reply_quality_bar>

<mood_logging_rules>
- Log mood only when the message is specific enough to infer happiness, energy, and stress with confidence.
- If mood is vague, choose chat and ask one short clarifying question.
</mood_logging_rules>

<progress_rules>
- Match the best goal from the supplied active goals when possible.
- Acknowledge progress warmly and connect it to the larger goal or to the user's wellbeing.
- If progress is qualitative, `value` may be null and the note should carry the detail.
</progress_rules>

<goal_creation_rules>
- Create a goal only when the user is expressing a durable intention, not a passing thought.
- Prefer concise, concrete titles.
- Capture the deeper reason in `why` when it is present or clearly implied.
</goal_creation_rules>

<grounding_rules>
- Base the decision and reply only on the current message plus the supplied context blocks.
- If something is an inference, keep it modest and reversible.
- Return exactly one JSON object matching the provided schema.
- Do not output markdown or prose outside the JSON object.
</grounding_rules>
"#;

fn intent_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "intent": {
                "type": "string",
                "enum": ["mood", "progress", "create_goal", "chat"]
            },
            "happiness": { "type": ["integer", "null"], "minimum": 1, "maximum": 10 },
            "energy": { "type": ["integer", "null"], "minimum": 1, "maximum": 10 },
            "stress": { "type": ["integer", "null"], "minimum": 1, "maximum": 10 },
            "note": { "type": ["string", "null"] },
            "goal_title": { "type": ["string", "null"] },
            "value": { "type": ["number", "null"] },
            "title": { "type": ["string", "null"] },
            "why": { "type": ["string", "null"] },
            "reply": { "type": ["string", "null"] }
        },
        "required": [
            "intent",
            "happiness",
            "energy",
            "stress",
            "note",
            "goal_title",
            "value",
            "title",
            "why",
            "reply"
        ],
        "additionalProperties": false
    })
}

fn parse_intent_from_content(content: &str) -> ParsedIntent {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return ParsedIntent::Chat {
            reply: FALLBACK_REPLY.to_string(),
        };
    }

    // Strip markdown code fences if the model wraps JSON output.
    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let unfenced = unfenced.strip_suffix("```").unwrap_or(unfenced).trim();

    if let Ok(parsed) = serde_json::from_str::<ParsedIntent>(unfenced) {
        return parsed;
    }

    if let Ok(parsed) = serde_json::from_str::<StructuredIntent>(unfenced) {
        if let Ok(intent) = ParsedIntent::try_from(parsed) {
            return intent;
        }
    }

    // Try to recover if the model adds extra prose around a JSON object.
    if let (Some(start), Some(end)) = (unfenced.find('{'), unfenced.rfind('}')) {
        if start < end {
            let candidate = &unfenced[start..=end];
            if let Ok(parsed) = serde_json::from_str::<ParsedIntent>(candidate) {
                return parsed;
            }
            if let Ok(parsed) = serde_json::from_str::<StructuredIntent>(candidate) {
                if let Ok(intent) = ParsedIntent::try_from(parsed) {
                    return intent;
                }
            }
        }
    }

    // Final fallback: treat raw model text as a normal chat reply.
    ParsedIntent::Chat {
        reply: if unfenced.is_empty() {
            FALLBACK_REPLY.to_string()
        } else {
            unfenced.to_string()
        },
    }
}

// parse_intent without memory is replaced by parse_intent_with_memory below

// ── Embeddings ──

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

pub async fn embed(texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
    let api_key = api_key().ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

    let resp = reqwest::Client::new()
        .post("https://api.openai.com/v1/embeddings")
        .bearer_auth(&api_key)
        .json(&serde_json::json!({
            "model": crate::config::embedding_model(),
            "input": texts,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI embeddings API {status}: {body}");
    }

    let emb_resp: EmbeddingResponse = resp.json().await?;
    Ok(emb_resp.data.into_iter().map(|d| d.embedding).collect())
}

// ── Observation generation ──

#[derive(Debug, Deserialize)]
pub struct GeneratedObservation {
    pub category: String,
    pub content: String,
    #[serde(default)]
    pub goal_title: Option<String>,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    #[serde(default)]
    pub supersedes: Option<String>,
}

fn default_confidence() -> f64 {
    0.8
}

const OBSERVATION_SYSTEM_PROMPT: &str = r#"<role>
You are the memory system for Happi, a wellbeing coaching agent.
</role>

<mission>
Generate durable observations that make future coaching more helpful, more personal, and more effective at improving the user's happiness through goal progress.
</mission>

<selection_rules>
- Only write observations that are likely to matter in future conversations.
- Focus on stable patterns, motivational drivers, risks, milestones, coaching preferences, and connections between mood, behavior, and goal progress.
- Prefer observations that would change how Happi coaches the user.
- Do not restate obvious one-off facts unless they signal a milestone or an important risk.
- If nothing durable or coaching-relevant stands out, return an empty array.
- Generate at most 3 observations.
</selection_rules>

<grounding_rules>
- Base every observation only on the supplied recent conversation, active goals, and existing observations.
- Do not invent diagnoses, hidden motives, or unsupported life facts.
- If an observation is tentative, reflect that in the confidence score rather than overstating it.
- Use `supersedes` only when the new observation clearly refines or replaces an existing one.
</grounding_rules>

<quality_bar>
- Keep each observation concise and specific.
- Favor observations that connect happiness, stress, energy, habits, and goal follow-through.
- Note communication-style preferences when they meaningfully affect coaching.
- Return exactly the JSON array required by the schema, with no markdown or extra prose.
</quality_bar>
"#;

fn observations_json_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": 3,
        "items": {
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": ["pattern", "insight", "preference", "risk", "milestone", "connection"]
                },
                "content": { "type": "string", "minLength": 1 },
                "goal_title": { "type": ["string", "null"] },
                "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                "supersedes": { "type": ["string", "null"] }
            },
            "required": ["category", "content", "goal_title", "confidence", "supersedes"],
            "additionalProperties": false
        }
    })
}

pub async fn generate_observations(
    recent_chat: &[(String, String)],
    existing_observations: &[(String, String, Option<String>, String)], // id, category, goal_id, content
    goal_titles: &[String],
) -> anyhow::Result<Vec<GeneratedObservation>> {
    let mut instructions = OBSERVATION_SYSTEM_PROMPT.to_string();

    if !goal_titles.is_empty() {
        instructions.push_str("\n<active_goals>\n");
        for (i, g) in goal_titles.iter().enumerate() {
            instructions.push_str(&format!("{}. {g}\n", i + 1));
        }
        instructions.push_str("</active_goals>\n");
    }

    if !existing_observations.is_empty() {
        instructions.push_str("\n<existing_observations>\n");
        for (id, category, _goal_id, content) in existing_observations {
            instructions.push_str(&format!("- [{category}] (id: {id}) {content}\n"));
        }
        instructions.push_str("</existing_observations>\n");
    }

    let mut chat_text = String::new();
    for (role, content) in recent_chat {
        chat_text.push_str(&format!("{role}: {content}\n"));
    }

    let content = create_structured_response(
        crate::config::observation_model(),
        instructions,
        vec![input_text_message(
            "user",
            format!(
                "<recent_conversation>\n{}</recent_conversation>\n\nReturn the structured observation array.",
                chat_text.trim()
            ),
        )],
        crate::config::observation_reasoning_effort(),
        crate::config::observation_verbosity(),
        "happi_observations",
        observations_json_schema(),
    )
        .await?;

    let observations: Vec<GeneratedObservation> = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse structured observations payload: {content}"))?;

    Ok(observations)
}

// ── Intent parsing with memory context ──

pub async fn parse_intent_with_memory(
    user_message: &str,
    history: &[(String, String)],
    user_goals: &[String],
    observations: &[(String, Option<String>, String, String)], // category, goal_title, content, created_at
    retrieved_context: &[String],
) -> anyhow::Result<ParsedIntent> {
    let mut instructions = SYSTEM_PROMPT.to_string();

    if !observations.is_empty() {
        instructions.push_str("\n<active_observations>\n");
        for (category, goal_title, content, created_at) in observations {
            let date = created_at.split('T').next().unwrap_or("?");
            let scope = goal_title
                .as_deref()
                .map(|g| format!(" (re: {g})"))
                .unwrap_or_default();
            instructions.push_str(&format!("- [{date}] [{category}]{scope} {content}\n"));
        }
        instructions.push_str("</active_observations>\n");
    }

    if !retrieved_context.is_empty() {
        instructions.push_str("\n<retrieved_context>\n");
        for ctx in retrieved_context {
            instructions.push_str(&format!("{ctx}\n"));
        }
        instructions.push_str("</retrieved_context>\n");
    }

    if !user_goals.is_empty() {
        instructions.push_str("\n<active_goals>\n");
        for (i, g) in user_goals.iter().enumerate() {
            instructions.push_str(&format!("{}. {g}\n", i + 1));
        }
        instructions.push_str("</active_goals>\n");
    }

    let mut input = Vec::with_capacity(history.len() + 1);

    for (role, content) in history {
        input.push(input_text_message(
            normalize_history_role(role),
            content.clone(),
        ));
    }

    input.push(input_text_message("user", user_message.to_string()));

    let content = create_structured_response(
        crate::config::chat_model(),
        instructions,
        input,
        crate::config::chat_reasoning_effort(),
        crate::config::chat_verbosity(),
        "happi_intent",
        intent_json_schema(),
    )
    .await?;

    let parsed = parse_intent_from_content(&content);
    tracing::debug!(
        raw_content = %content,
        parsed_intent = ?parsed,
        "parsed intent with memory"
    );
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{
        FALLBACK_REPLY, ParsedIntent, ResponseApiResponse, extract_output_text,
        parse_intent_from_content,
    };

    #[test]
    fn parses_valid_json() {
        let parsed = parse_intent_from_content(
            r#"{"intent":"chat","reply":"Hello! How can I support you today?"}"#,
        );
        match parsed {
            ParsedIntent::Chat { reply } => {
                assert_eq!(reply, "Hello! How can I support you today?");
            }
            _ => panic!("expected chat intent"),
        }
    }

    #[test]
    fn parses_fenced_json() {
        let parsed = parse_intent_from_content(
            "```json\n{\"intent\":\"create_goal\",\"title\":\"Read daily\",\"why\":\"Focus\"}\n```",
        );
        match parsed {
            ParsedIntent::CreateGoal { title, why } => {
                assert_eq!(title, "Read daily");
                assert_eq!(why.as_deref(), Some("Focus"));
            }
            _ => panic!("expected create_goal intent"),
        }
    }

    #[test]
    fn parses_json_with_extra_text() {
        let parsed = parse_intent_from_content(
            "Sure, here's the result:\n{\"intent\":\"chat\",\"reply\":\"I can help with that.\"}",
        );
        match parsed {
            ParsedIntent::Chat { reply } => {
                assert_eq!(reply, "I can help with that.");
            }
            _ => panic!("expected chat intent"),
        }
    }

    #[test]
    fn parses_flat_structured_intent_payload() {
        let parsed = parse_intent_from_content(
            r#"{"intent":"progress","happiness":null,"energy":null,"stress":null,"note":"Completed today's run","goal_title":"Run 3x/week","value":null,"title":null,"why":null,"reply":null}"#,
        );
        match parsed {
            ParsedIntent::Progress {
                goal_title,
                value,
                note,
            } => {
                assert_eq!(goal_title, "Run 3x/week");
                assert_eq!(value, None);
                assert_eq!(note.as_deref(), Some("Completed today's run"));
            }
            _ => panic!("expected progress intent"),
        }
    }

    #[test]
    fn falls_back_to_chat_on_plain_text() {
        let parsed = parse_intent_from_content("You have 2 active goals. Want a summary?");
        match parsed {
            ParsedIntent::Chat { reply } => {
                assert_eq!(reply, "You have 2 active goals. Want a summary?");
            }
            _ => panic!("expected chat intent"),
        }
    }

    #[test]
    fn falls_back_on_empty_output() {
        let parsed = parse_intent_from_content("   ");
        match parsed {
            ParsedIntent::Chat { reply } => {
                assert_eq!(reply, FALLBACK_REPLY);
            }
            _ => panic!("expected chat intent"),
        }
    }

    #[test]
    fn extracts_output_text_from_responses_payload() {
        let payload = serde_json::json!({
            "output": [
                { "type": "reasoning", "summary": [] },
                {
                    "type": "message",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "{\"intent\":\"chat\",\"reply\":\"ok\"}"
                        }
                    ]
                }
            ]
        });
        let response: ResponseApiResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(
            extract_output_text(&response),
            Some("{\"intent\":\"chat\",\"reply\":\"ok\"}")
        );
    }
}
