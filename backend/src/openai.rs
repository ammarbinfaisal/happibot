use anyhow::Context;
use chrono::Utc;
use reqwest::multipart;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

fn api_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}

// ── Whisper transcription ──

pub async fn transcribe(file_bytes: Vec<u8>, filename: &str) -> anyhow::Result<String> {
    let api_key = api_key().ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;
    let audio_bytes = file_bytes.len();
    let start = Instant::now();

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
        tracing::warn!(
            model = "whisper-1",
            filename,
            audio_bytes,
            duration_ms = start.elapsed().as_millis() as u64,
            status = %status,
            "openai transcription failed"
        );
        anyhow::bail!("Whisper API {status}: {body}");
    }

    let transcript = resp.text().await?.trim().to_string();
    tracing::info!(
        model = "whisper-1",
        filename,
        audio_bytes,
        transcript_chars = transcript.chars().count(),
        duration_ms = start.elapsed().as_millis() as u64,
        "openai transcription completed"
    );
    Ok(transcript)
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
    #[serde(rename = "type")]
    message_type: String,
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
    id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    content: Vec<ResponseOutputContent>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct ResponseOutputContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

fn input_text_message(role: impl Into<String>, text: impl Into<String>) -> ResponseInputMessage {
    ResponseInputMessage {
        message_type: "message".to_string(),
        role: role.into(),
        content: vec![ResponseInputContent {
            content_type: "input_text".to_string(),
            text: text.into(),
        }],
    }
}

fn output_text_message(role: impl Into<String>, text: impl Into<String>) -> ResponseInputMessage {
    ResponseInputMessage {
        message_type: "message".to_string(),
        role: role.into(),
        content: vec![ResponseInputContent {
            content_type: "output_text".to_string(),
            text: text.into(),
        }],
    }
}

fn history_message(role: &str, text: impl Into<String>) -> ResponseInputMessage {
    match role {
        "assistant" => output_text_message("assistant", text),
        "system" => input_text_message("system", text),
        "developer" => input_text_message("developer", text),
        _ => input_text_message("user", text),
    }
}

fn normalize_openai_schema(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            map.remove("$schema");
            map.remove("title");
            map.remove("description");
            map.remove("default");
            map.remove("examples");

            if let Some(props) = map.get_mut("properties").and_then(Value::as_object_mut) {
                for property_schema in props.values_mut() {
                    normalize_openai_schema(property_schema);
                }

                let required = props.keys().cloned().map(Value::String).collect::<Vec<_>>();
                map.insert("required".to_string(), Value::Array(required));
                map.insert("additionalProperties".to_string(), Value::Bool(false));
            }

            for key in [
                "items",
                "additionalProperties",
                "contains",
                "if",
                "then",
                "else",
                "not",
                "propertyNames",
            ] {
                if let Some(value) = map.get_mut(key) {
                    normalize_openai_schema(value);
                }
            }

            for key in ["anyOf", "allOf", "oneOf", "prefixItems"] {
                if let Some(values) = map.get_mut(key).and_then(Value::as_array_mut) {
                    for value in values {
                        normalize_openai_schema(value);
                    }
                }
            }

            for key in [
                "$defs",
                "definitions",
                "patternProperties",
                "dependentSchemas",
            ] {
                if let Some(values) = map.get_mut(key).and_then(Value::as_object_mut) {
                    for value in values.values_mut() {
                        normalize_openai_schema(value);
                    }
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_openai_schema(value);
            }
        }
        _ => {}
    }
}

fn openai_json_schema_for<T: JsonSchema>() -> Value {
    let mut schema =
        serde_json::to_value(schema_for!(T)).expect("schema generation should succeed");
    normalize_openai_schema(&mut schema);
    schema
}

fn json_schema_format(name: &str, schema: Value) -> anyhow::Result<Value> {
    let root_type = schema.get("type").and_then(Value::as_str);
    if root_type != Some("object") {
        anyhow::bail!(
            "OpenAI structured outputs require a root object schema, got {:?}",
            root_type.unwrap_or("unknown")
        );
    }

    Ok(json!({
        "type": "json_schema",
        "name": name,
        "schema": schema,
        "strict": true,
    }))
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

fn input_text_char_count(input: &[ResponseInputMessage]) -> usize {
    input
        .iter()
        .flat_map(|message| message.content.iter())
        .map(|content| content.text.chars().count())
        .sum()
}

fn output_text_char_count(response: &ResponseApiResponse) -> usize {
    extract_output_text(response)
        .map(|text| text.chars().count())
        .unwrap_or(0)
}

#[derive(Debug, PartialEq, Eq)]
struct ToolCallLogEntry {
    item_type: String,
    call_id: Option<String>,
    tool_name: Option<String>,
    status: Option<String>,
}

fn response_tool_call_entries(resp: &ResponseApiResponse) -> Vec<ToolCallLogEntry> {
    resp.output
        .iter()
        .filter(|item| is_tool_call_item(item))
        .map(|item| ToolCallLogEntry {
            item_type: item.item_type.clone(),
            call_id: item.call_id.clone().or_else(|| item.id.clone()),
            tool_name: item
                .name
                .clone()
                .or_else(|| string_field(&item.extra, "tool_name"))
                .or_else(|| string_field(&item.extra, "server_label")),
            status: item.status.clone(),
        })
        .collect()
}

fn is_tool_call_item(item: &ResponseOutputItem) -> bool {
    item.item_type.ends_with("_call") || item.item_type == "custom_tool_call"
}

fn string_field(extra: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    extra.get(key)?.as_str().map(str::to_string)
}

fn log_response_tool_calls(
    model: &str,
    schema_name: &str,
    response: &ResponseApiResponse,
    request_duration_ms: u64,
) {
    for entry in response_tool_call_entries(response) {
        tracing::info!(
            timestamp = %Utc::now().to_rfc3339(),
            model,
            schema_name,
            request_duration_ms,
            tool_call_type = %entry.item_type,
            tool_call_id = entry.call_id.as_deref().unwrap_or("-"),
            tool_name = entry.tool_name.as_deref().unwrap_or("-"),
            tool_status = entry.status.as_deref().unwrap_or("-"),
            "openai tool call"
        );
    }
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
    let input_message_count = input.len();
    let input_char_count = input_text_char_count(&input);
    let start = Instant::now();

    let req = ResponseRequest {
        model: model.clone(),
        instructions,
        input,
        reasoning: ResponseReasoning {
            effort: reasoning_effort,
        },
        text: ResponseTextConfig {
            verbosity,
            format: json_schema_format(schema_name, schema)?,
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
        tracing::warn!(
            model,
            schema_name,
            input_message_count,
            input_char_count,
            duration_ms = start.elapsed().as_millis() as u64,
            status = %status,
            "openai structured response failed"
        );
        anyhow::bail!("OpenAI responses API {status}: {body}");
    }

    let response: ResponseApiResponse = resp
        .json()
        .await
        .context("failed to decode OpenAI responses payload")?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let tool_call_count = response_tool_call_entries(&response).len();
    tracing::info!(
        model,
        schema_name,
        input_message_count,
        input_char_count,
        output_items = response.output.len(),
        output_char_count = output_text_char_count(&response),
        tool_call_count,
        duration_ms,
        "openai structured response completed"
    );

    log_response_tool_calls(&model, schema_name, &response, duration_ms);

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
    CreateGoal {
        title: String,
        why: String,
        cadence: String,
    },
    #[serde(rename = "delete_goal")]
    DeleteGoal { goal_title: String },
    #[serde(rename = "chat")]
    Chat { reply: String },
}

#[derive(Debug, Deserialize, JsonSchema)]
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
    cadence: Option<String>,
    #[serde(default)]
    reply: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ObservationCategorySchema {
    Pattern,
    Insight,
    Preference,
    Risk,
    Milestone,
    Connection,
}

impl ObservationCategorySchema {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pattern => "pattern",
            Self::Insight => "insight",
            Self::Preference => "preference",
            Self::Risk => "risk",
            Self::Milestone => "milestone",
            Self::Connection => "connection",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StructuredObservation {
    category: ObservationCategorySchema,
    content: String,
    goal_title: Option<String>,
    #[serde(default = "default_confidence")]
    confidence: f64,
    supersedes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StructuredObservations {
    observations: Vec<StructuredObservation>,
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
                why: value
                    .why
                    .context("structured intent missing `why` for create_goal")?,
                cadence: value
                    .cadence
                    .context("structured intent missing `cadence` for create_goal")?,
            }),
            "delete_goal" => Ok(Self::DeleteGoal {
                goal_title: value
                    .goal_title
                    .context("structured intent missing `goal_title` for delete_goal")?,
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
- Prefer the minimal useful response or action. Do not add extra steps, options, or internal process.
</default_follow_through_policy>

<decision_policy>
Choose exactly one intent:
1. mood: the user is reporting happiness, energy, or stress specifically enough to log.
2. progress: the user is updating progress on a known goal or a clearly implied goal.
3. create_goal: the user is defining a new goal or clearly asking to set one.
4. delete_goal: the user is clearly asking to delete, remove, stop tracking, or archive an active goal.
5. chat: everything else, including messages that need support, coaching, or clarification.

When deciding between logging and coaching:
- Prioritize emotional attunement when the user seems stressed, ashamed, discouraged, overwhelmed, or vulnerable.
- Prioritize progress reinforcement when the user made progress, even if small.
- If the user sounds stuck, lower the bar and guide them toward the smallest meaningful next step.
- If the user has no clear goal but wants change, help them define a goal that is concrete and personally meaningful.
- Do not force tracking when the user mainly needs reflection, encouragement, or a clarifying question.
- If required fields for `mood`, `progress`, `create_goal`, or `delete_goal` are missing, choose `chat` instead and put the one short clarifying question in `reply`.
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
- Capture the deeper reason in `why`.
- Capture the user's expected working rhythm in `cadence`, such as "daily", "3 times a week", or "weekends".
- If the user names a goal without enough detail to create it well, ask one short question that requests both motivation and cadence in the same sentence.
- Never choose `create_goal` when the goal title is missing or still ambiguous.
</goal_creation_rules>

<goal_deletion_rules>
- Choose `delete_goal` only when the user is clearly asking to remove an active goal.
- Match the best active goal title from context when possible.
- If it is unclear which goal should be deleted, choose `chat` and ask one short clarifying question.
</goal_deletion_rules>

<grounding_rules>
- Base the decision and reply only on the current message plus the supplied context blocks.
- If something is an inference, keep it modest and reversible.
- Return exactly one JSON object matching the provided schema.
- Do not output markdown or prose outside the JSON object.
- For `chat`, `reply` must contain the user-facing message.
- For `mood`, `progress`, `create_goal`, and `delete_goal`, `reply` must be null.
</grounding_rules>
"#;

fn intent_json_schema() -> Value {
    openai_json_schema_for::<StructuredIntent>()
}

fn parse_intent_value(value: Value) -> Option<ParsedIntent> {
    match value {
        Value::String(inner) => parse_intent_candidate(&inner),
        Value::Object(_) => {
            if let Ok(parsed) = serde_json::from_value::<ParsedIntent>(value.clone()) {
                return Some(parsed);
            }

            if let Ok(parsed) = serde_json::from_value::<StructuredIntent>(value) {
                let fallback_reply = parsed
                    .reply
                    .clone()
                    .filter(|reply| !reply.trim().is_empty());
                if let Ok(intent) = ParsedIntent::try_from(parsed) {
                    return Some(intent);
                }
                if let Some(reply) = fallback_reply {
                    return Some(ParsedIntent::Chat { reply });
                }
            }

            None
        }
        _ => None,
    }
}

fn parse_intent_candidate(candidate: &str) -> Option<ParsedIntent> {
    let value: Value = serde_json::from_str(candidate).ok()?;
    parse_intent_value(value)
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

    if let Some(parsed) = parse_intent_candidate(unfenced) {
        return parsed;
    }

    // Try to recover if the model adds extra prose around a JSON object.
    if let (Some(start), Some(end)) = (unfenced.find('{'), unfenced.rfind('}')) {
        if start < end {
            let candidate = &unfenced[start..=end];
            if let Some(parsed) = parse_intent_candidate(candidate) {
                return parsed;
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
    let model = crate::config::embedding_model();
    let input_count = texts.len();
    let input_char_count = texts.iter().map(|text| text.chars().count()).sum::<usize>();
    let start = Instant::now();

    let resp = reqwest::Client::new()
        .post("https://api.openai.com/v1/embeddings")
        .bearer_auth(&api_key)
        .json(&serde_json::json!({
            "model": model.clone(),
            "input": texts,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(
            model,
            input_count,
            input_char_count,
            duration_ms = start.elapsed().as_millis() as u64,
            status = %status,
            "openai embeddings failed"
        );
        anyhow::bail!("OpenAI embeddings API {status}: {body}");
    }

    let emb_resp: EmbeddingResponse = resp.json().await?;
    let embeddings: Vec<Vec<f32>> = emb_resp.data.into_iter().map(|d| d.embedding).collect();
    tracing::info!(
        model,
        input_count,
        input_char_count,
        output_count = embeddings.len(),
        embedding_dimensions = embeddings
            .first()
            .map(|embedding| embedding.len())
            .unwrap_or(0),
        duration_ms = start.elapsed().as_millis() as u64,
        "openai embeddings completed"
    );
    Ok(embeddings)
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
- If nothing durable or coaching-relevant stands out, return an empty `observations` array.
- Prefer 0-1 observations. Use 2-3 only when multiple clearly distinct durable observations are strongly supported.
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
- Return exactly the JSON object required by the schema, with no markdown or extra prose.
</quality_bar>
"#;

fn observations_json_schema() -> Value {
    let mut schema = openai_json_schema_for::<StructuredObservations>();
    if let Some(observations) = schema.pointer_mut("/properties/observations") {
        if let Some(map) = observations.as_object_mut() {
            map.insert("maxItems".to_string(), json!(3));
        }
    }
    schema
}

fn parse_observations_from_content(content: &str) -> anyhow::Result<Vec<GeneratedObservation>> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let unfenced = unfenced.strip_suffix("```").unwrap_or(unfenced).trim();

    if let Ok(parsed) = serde_json::from_str::<StructuredObservations>(unfenced) {
        return Ok(parsed
            .observations
            .into_iter()
            .map(|observation| GeneratedObservation {
                category: observation.category.as_str().to_string(),
                content: observation.content,
                goal_title: observation.goal_title,
                confidence: observation.confidence,
                supersedes: observation.supersedes,
            })
            .collect());
    }

    if let Ok(parsed) = serde_json::from_str::<Vec<GeneratedObservation>>(unfenced) {
        return Ok(parsed);
    }

    if let (Some(start), Some(end)) = (unfenced.find('{'), unfenced.rfind('}')) {
        if start < end {
            let candidate = &unfenced[start..=end];
            if let Ok(parsed) = serde_json::from_str::<StructuredObservations>(candidate) {
                return Ok(parsed
                    .observations
                    .into_iter()
                    .map(|observation| GeneratedObservation {
                        category: observation.category.as_str().to_string(),
                        content: observation.content,
                        goal_title: observation.goal_title,
                        confidence: observation.confidence,
                        supersedes: observation.supersedes,
                    })
                    .collect());
            }
        }
    }

    if let (Some(start), Some(end)) = (unfenced.find('['), unfenced.rfind(']')) {
        if start < end {
            let candidate = &unfenced[start..=end];
            if let Ok(parsed) = serde_json::from_str::<Vec<GeneratedObservation>>(candidate) {
                return Ok(parsed);
            }
        }
    }

    anyhow::bail!("failed to parse structured observations payload: {content}")
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
                "<recent_conversation>\n{}</recent_conversation>\n\nReturn the structured observations object.",
                chat_text.trim()
            ),
        )],
        crate::config::observation_reasoning_effort(),
        crate::config::observation_verbosity(),
        "happi_observations",
        observations_json_schema(),
    )
        .await?;

    parse_observations_from_content(&content)
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
        input.push(history_message(role, content.clone()));
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
        FALLBACK_REPLY, ParsedIntent, ResponseApiResponse, ToolCallLogEntry, extract_output_text,
        history_message, intent_json_schema, observations_json_schema, parse_intent_from_content,
        parse_observations_from_content, response_tool_call_entries,
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
            "```json\n{\"intent\":\"create_goal\",\"title\":\"Read daily\",\"why\":\"Focus\",\"cadence\":\"daily\"}\n```",
        );
        match parsed {
            ParsedIntent::CreateGoal {
                title,
                why,
                cadence,
            } => {
                assert_eq!(title, "Read daily");
                assert_eq!(why, "Focus");
                assert_eq!(cadence, "daily");
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
    fn falls_back_to_chat_reply_when_non_chat_intent_is_missing_required_fields() {
        let parsed = parse_intent_from_content(
            r#"{"energy":null,"goal_title":null,"happiness":null,"intent":"create_goal","note":null,"reply":"What goal do you want to add, why does it matter to you, and how often will you work on it?","stress":null,"title":null,"value":null,"why":null,"cadence":null}"#,
        );
        match parsed {
            ParsedIntent::Chat { reply } => {
                assert_eq!(
                    reply,
                    "What goal do you want to add, why does it matter to you, and how often will you work on it?"
                );
            }
            _ => panic!("expected chat intent"),
        }
    }

    #[test]
    fn parses_json_string_wrapped_intent_payload() {
        let parsed = parse_intent_from_content(
            r#""{\"intent\":\"create_goal\",\"title\":\"Run a 5k\",\"why\":\"More energy\",\"cadence\":\"4 times a week\"}""#,
        );
        match parsed {
            ParsedIntent::CreateGoal {
                title,
                why,
                cadence,
            } => {
                assert_eq!(title, "Run a 5k");
                assert_eq!(why, "More energy");
                assert_eq!(cadence, "4 times a week");
            }
            _ => panic!("expected create_goal intent"),
        }
    }

    #[test]
    fn parses_delete_goal_payload() {
        let parsed = parse_intent_from_content(
            r#"{"intent":"delete_goal","goal_title":"Run 3x/week","reply":null}"#,
        );
        match parsed {
            ParsedIntent::DeleteGoal { goal_title } => {
                assert_eq!(goal_title, "Run 3x/week");
            }
            _ => panic!("expected delete_goal intent"),
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

    #[test]
    fn extracts_tool_calls_from_responses_payload() {
        let payload = serde_json::json!({
            "output": [
                { "type": "reasoning", "summary": [] },
                {
                    "type": "web_search_call",
                    "id": "ws_123",
                    "status": "completed"
                },
                {
                    "type": "function_call",
                    "call_id": "call_456",
                    "name": "save_goal",
                    "status": "completed",
                    "arguments": "{\"title\":\"Run\"}"
                },
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
            response_tool_call_entries(&response),
            vec![
                ToolCallLogEntry {
                    item_type: "web_search_call".to_string(),
                    call_id: Some("ws_123".to_string()),
                    tool_name: None,
                    status: Some("completed".to_string()),
                },
                ToolCallLogEntry {
                    item_type: "function_call".to_string(),
                    call_id: Some("call_456".to_string()),
                    tool_name: Some("save_goal".to_string()),
                    status: Some("completed".to_string()),
                }
            ]
        );
    }

    #[test]
    fn serializes_assistant_history_as_output_text_message() {
        let message = history_message("assistant", "Previous reply");
        let value = serde_json::to_value(message).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Previous reply"
                    }
                ]
            })
        );
    }

    #[test]
    fn intent_schema_has_object_root() {
        let schema = intent_json_schema();
        assert_eq!(
            schema.get("type").and_then(|value| value.as_str()),
            Some("object")
        );
        assert_eq!(
            schema
                .get("additionalProperties")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn observations_schema_wraps_array_in_object() {
        let schema = observations_json_schema();
        assert_eq!(
            schema.get("type").and_then(|value| value.as_str()),
            Some("object")
        );
        assert_eq!(
            schema
                .pointer("/properties/observations/type")
                .and_then(|value| value.as_str()),
            Some("array")
        );
        assert_eq!(
            schema
                .pointer("/properties/observations/maxItems")
                .and_then(|value| value.as_u64()),
            Some(3)
        );
    }

    #[test]
    fn parses_wrapped_observations_payload() {
        let observations = parse_observations_from_content(
            r#"{"observations":[{"category":"pattern","content":"Late workouts improve mood","goal_title":"Exercise","confidence":0.9,"supersedes":null}]}"#,
        )
        .unwrap();

        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].category, "pattern");
        assert_eq!(observations[0].content, "Late workouts improve mood");
    }

    #[test]
    fn parses_legacy_array_observations_payload() {
        let observations = parse_observations_from_content(
            r#"[{"category":"risk","content":"Skips routines when sleep is poor","goal_title":null,"confidence":0.7,"supersedes":null}]"#,
        )
        .unwrap();

        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].category, "risk");
    }

    #[test]
    fn wrapped_observations_default_confidence_when_missing() {
        let observations = parse_observations_from_content(
            r#"{"observations":[{"category":"preference","content":"Prefers short check-ins","goal_title":null,"supersedes":null}]}"#,
        )
        .unwrap();

        assert_eq!(observations.len(), 1);
        assert!((observations[0].confidence - 0.8).abs() < f64::EPSILON);
    }
}
