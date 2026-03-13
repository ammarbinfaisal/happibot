use reqwest::multipart;
use serde::{Deserialize, Serialize};

fn api_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.is_empty())
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

// ── Chat completion for intent parsing ──

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
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
        why: Option<String>,
    },
    #[serde(rename = "chat")]
    Chat {
        reply: String,
    },
}

const FALLBACK_REPLY: &str = "I'm having trouble thinking right now. Try again?";

const SYSTEM_PROMPT: &str = r#"You are Happi, a friendly wellbeing & goals coaching bot on Telegram.
The user sends you natural-language messages (sometimes transcribed from voice).
Parse the user's intent and respond with a single JSON object (no markdown, no extra text).

Possible intents:

1. Mood check-in: the user reports how they feel.
   {"intent":"mood","happiness":<1-10>,"energy":<1-10>,"stress":<1-10>,"note":"<optional short note>"}
   Infer values from context. If they say "I feel great, lots of energy, not stressed" → high happiness, high energy, low stress.

2. Progress update: the user reports progress on a goal.
   {"intent":"progress","goal_title":"<best guess of goal name>","value":<optional number>,"note":"<optional note>"}

3. Create a new goal:
   {"intent":"create_goal","title":"<goal title>","why":"<optional reason>"}

4. General chat / anything else:
   {"intent":"chat","reply":"<your helpful, warm, concise reply as Happi the coach>"}
   Use this for greetings, questions, motivation requests, or anything that doesn't fit above.

Rules:
- Always output valid JSON, nothing else.
- For mood values, use your best judgment to map qualitative descriptions to 1-10 scales.
- Be warm and encouraging in chat replies. Keep them short (1-3 sentences).
- If the message is ambiguous, prefer "chat" intent and ask a clarifying question.
- If the user mentions a mood but is vague (e.g. "not great"), use "chat" to ask 1-2 short clarifying questions before logging. For example: "I hear you. Can you tell me a bit more — how's your energy, and what's stressing you?"
- You'll see the user's active goals in the system context. Use them to match progress updates.
"#;

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

    // Try to recover if the model adds extra prose around a JSON object.
    if let (Some(start), Some(end)) = (unfenced.find('{'), unfenced.rfind('}')) {
        if start < end {
            let candidate = &unfenced[start..=end];
            if let Ok(parsed) = serde_json::from_str::<ParsedIntent>(candidate) {
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

pub async fn parse_intent(
    user_message: &str,
    history: &[(String, String)], // (role, content) pairs
    user_goals: &[String],       // active goal titles for context
) -> anyhow::Result<ParsedIntent> {
    let api_key = api_key().ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

    let mut system_content = SYSTEM_PROMPT.to_string();
    if !user_goals.is_empty() {
        system_content.push_str("\nThe user's active goals:\n");
        for (i, g) in user_goals.iter().enumerate() {
            system_content.push_str(&format!("{}. {g}\n", i + 1));
        }
    }

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: system_content,
    }];

    // Add recent conversation history for context
    for (role, content) in history {
        messages.push(ChatMessage {
            role: role.clone(),
            content: content.clone(),
        });
    }

    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_message.to_string(),
    });

    let req = ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages,
        temperature: 0.3,
    };

    let resp = reqwest::Client::new()
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(&api_key)
        .json(&req)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI chat API {status}: {body}");
    }

    let chat_resp: ChatResponse = resp.json().await?;
    let content = chat_resp
        .choices
        .first()
        .and_then(|c| c.message.content.as_deref())
        .unwrap_or("");
    let parsed = parse_intent_from_content(content);
    tracing::debug!(
        raw_content = %content,
        parsed_intent = ?parsed,
        "parsed OpenAI intent response"
    );
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{parse_intent_from_content, ParsedIntent, FALLBACK_REPLY};

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
}
