use std::fmt;

#[derive(Debug)]
pub enum OllamaError {
    ConnectionRefused,
    Timeout,
    ModelNotFound(String),
    Other(String),
}

impl fmt::Display for OllamaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OllamaError::ConnectionRefused => write!(f, "Ollama not running — start with: systemctl start ollama"),
            OllamaError::Timeout => write!(f, "Correction timed out — try selecting fewer lines"),
            OllamaError::ModelNotFound(m) => write!(f, "Model not found — run: ollama pull {}", m),
            OllamaError::Other(msg) => write!(f, "Ollama error: {}", msg),
        }
    }
}

const SYSTEM_PROMPT: &str = "\
You are correcting mistranscribed audiobook text. \
Fix ONLY words that are obviously wrong due to speech-to-text mishearing \
(homophones, phonetically similar but wrong words). \
Do NOT rephrase, restructure, or improve the text. \
Preserve original line breaks exactly. \
Output ONLY the corrected text with no commentary.";

pub async fn correct_text(
    endpoint: &str,
    model: &str,
    text: &str,
) -> Result<String, OllamaError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| OllamaError::Other(e.to_string()))?;

    let url = format!("{}/api/generate", endpoint);

    let body = serde_json::json!({
        "model": model,
        "system": SYSTEM_PROMPT,
        "prompt": text,
        "stream": false
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() {
                OllamaError::ConnectionRefused
            } else if e.is_timeout() {
                OllamaError::Timeout
            } else {
                OllamaError::Other(e.to_string())
            }
        })?;

    let status = response.status();
    let text = response.text().await.map_err(|e| OllamaError::Other(e.to_string()))?;

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(OllamaError::ModelNotFound(model.to_string()));
    }

    if !status.is_success() {
        return Err(OllamaError::Other(format!("HTTP {}: {}", status, text)));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| OllamaError::Other(e.to_string()))?;

    json.get("response")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| OllamaError::Other("No 'response' field in Ollama output".to_string()))
}
