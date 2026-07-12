use std::fmt;

#[derive(Debug)]
pub enum ClaudeError {
    MissingApiKey,
    Unauthorized,
    Timeout,
    RateLimited,
    ApiError(String),
}

impl fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaudeError::MissingApiKey => write!(f, "Set ANTHROPIC_API_KEY environment variable"),
            ClaudeError::Unauthorized => write!(
                f,
                "Invalid ANTHROPIC_API_KEY — check the key and relaunch"
            ),
            ClaudeError::Timeout => write!(f, "Request timed out — try selecting fewer lines"),
            ClaudeError::RateLimited => write!(f, "Rate limited — try again in a moment"),
            ClaudeError::ApiError(msg) => write!(f, "API error: {}", msg),
        }
    }
}

/// Pull the human-readable `error.message` out of an Anthropic error body,
/// falling back to a truncated raw body if it isn't the expected shape.
fn extract_api_error(status: reqwest::StatusCode, text: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        });
    match message {
        Some(msg) => format!("HTTP {} — {}", status.as_u16(), msg),
        None => {
            let snippet: String = text.chars().take(200).collect();
            format!("HTTP {}: {}", status, snippet)
        }
    }
}

/// One conversation turn for `send_chat`. `role` is "user" or "assistant".
#[derive(Clone, Debug)]
pub struct ChatTurn {
    pub role: &'static str,
    pub content: String,
}

/// Build the /v1/messages request body for a multi-turn conversation.
/// Kept as a pure fn so the shape is unit-testable.
///
/// Prompt caching: the system prompt is stable across requests, and a chat
/// conversation's history is an append-only prefix — both get `cache_control`
/// breakpoints so repeat sends bill at the cached-read rate (prefixes under
/// the model's cacheable minimum are silently not cached, which is safe).
/// The final-message breakpoint is only set for multi-turn requests: caching
/// a one-shot's user message writes an entry that can never be read back.
fn chat_body(system: &str, turns: &[ChatTurn], model: &str) -> serde_json::Value {
    let n = turns.len();
    let messages: Vec<serde_json::Value> = turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if n >= 2 && i == n - 1 {
                serde_json::json!({"role": t.role, "content": [{
                    "type": "text",
                    "text": t.content,
                    "cache_control": {"type": "ephemeral"},
                }]})
            } else {
                serde_json::json!({"role": t.role, "content": t.content})
            }
        })
        .collect();
    serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "system": [{
            "type": "text",
            "text": system,
            "cache_control": {"type": "ephemeral"},
        }],
        "messages": messages,
    })
}

/// Multi-turn variant of `send_message`: `turns` carries the session's prior
/// user/assistant messages plus the new user message, in order.
pub async fn send_chat(
    system: &str,
    turns: &[ChatTurn],
    model: &str,
) -> Result<String, ClaudeError> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| ClaudeError::MissingApiKey)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ClaudeError::ApiError(e.to_string()))?;

    let body = chat_body(system, turns, model);

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ClaudeError::Timeout
            } else {
                ClaudeError::ApiError(e.to_string())
            }
        })?;

    let status = response.status();
    let text = response.text().await.map_err(|e| ClaudeError::ApiError(e.to_string()))?;

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ClaudeError::RateLimited);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ClaudeError::Unauthorized);
    }
    if !status.is_success() {
        return Err(ClaudeError::ApiError(extract_api_error(status, &text)));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| ClaudeError::ApiError(e.to_string()))?;

    if let Some(u) = json.get("usage") {
        let g = |k: &str| u.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        crate::logging::log(&format!(
            "CLAUDE: usage in={} cache_read={} cache_write={} out={}",
            g("input_tokens"),
            g("cache_read_input_tokens"),
            g("cache_creation_input_tokens"),
            g("output_tokens"),
        ));
    }

    json.get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ClaudeError::ApiError("No text in response".to_string()))
}

pub async fn send_message(
    system: &str,
    user_message: &str,
    model: &str,
) -> Result<String, ClaudeError> {
    send_chat(
        system,
        &[ChatTurn { role: "user", content: user_message.to_string() }],
        model,
    )
    .await
}

#[cfg(test)]
mod chat_body_tests {
    use super::{chat_body, ChatTurn};

    fn turn(role: &'static str, content: &str) -> ChatTurn {
        ChatTurn { role, content: content.to_string() }
    }

    #[test]
    fn multi_turn_body_preserves_order_and_roles() {
        let turns = [
            turn("user", "q1"),
            turn("assistant", "a1"),
            turn("user", "q2"),
        ];
        let body = chat_body("SYS", &turns, "claude-opus-4-8");
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(body["max_tokens"], 4096);
        // System is a cache-marked content block.
        assert_eq!(body["system"][0]["text"], "SYS");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "q1");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "a1");
        // The final message of a multi-turn request carries the history
        // cache breakpoint, so it becomes a content-block array.
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"][0]["text"], "q2");
        assert_eq!(msgs[2]["content"][0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn single_turn_body_has_no_message_cache_breakpoint() {
        let body = chat_body("S", &[turn("user", "hello")], "m");
        // One-shot user content stays a plain string: caching it would
        // write an entry that is never read back.
        assert_eq!(
            body["messages"],
            serde_json::json!([{"role": "user", "content": "hello"}])
        );
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    }
}
