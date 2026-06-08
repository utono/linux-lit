use std::fmt;

#[derive(Debug)]
pub enum ElevenLabsError {
    MissingApiKey,
    Timeout,
    RateLimited,
    ApiError(String),
}

impl fmt::Display for ElevenLabsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevenLabsError::MissingApiKey => {
                write!(f, "Set ELEVENLABS_API_KEY environment variable")
            }
            ElevenLabsError::Timeout => write!(f, "TTS request timed out"),
            ElevenLabsError::RateLimited => write!(f, "TTS rate limited — try again"),
            ElevenLabsError::ApiError(msg) => write!(f, "TTS API error: {}", msg),
        }
    }
}

fn tts_url(voice_id: &str) -> String {
    format!("https://api.elevenlabs.io/v1/text-to-speech/{}", voice_id)
}

fn build_body(text: &str, model_id: &str) -> serde_json::Value {
    serde_json::json!({ "text": text, "model_id": model_id })
}

/// Synthesize `text` to MP3 bytes via ElevenLabs. Key from ELEVENLABS_API_KEY.
pub async fn synthesize(
    text: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<Vec<u8>, ElevenLabsError> {
    let api_key =
        std::env::var("ELEVENLABS_API_KEY").map_err(|_| ElevenLabsError::MissingApiKey)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ElevenLabsError::ApiError(e.to_string()))?;

    let response = client
        .post(tts_url(voice_id))
        .header("xi-api-key", &api_key)
        .header("accept", "audio/mpeg")
        .header("content-type", "application/json")
        .json(&build_body(text, model_id))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ElevenLabsError::Timeout
            } else {
                ElevenLabsError::ApiError(e.to_string())
            }
        })?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ElevenLabsError::RateLimited);
    }
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(ElevenLabsError::ApiError(format!("HTTP {}: {}", status, text)));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ElevenLabsError::ApiError(e.to_string()))?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        assert_eq!(
            ElevenLabsError::MissingApiKey.to_string(),
            "Set ELEVENLABS_API_KEY environment variable"
        );
        assert_eq!(
            ElevenLabsError::ApiError("boom".into()).to_string(),
            "TTS API error: boom"
        );
    }

    #[test]
    fn request_url_uses_voice_id() {
        assert_eq!(
            tts_url("21m00Tcm4TlvDq8ikWAM"),
            "https://api.elevenlabs.io/v1/text-to-speech/21m00Tcm4TlvDq8ikWAM"
        );
    }

    #[test]
    fn request_body_has_text_and_model() {
        let body = build_body("hello", "eleven_turbo_v2_5");
        assert_eq!(body["text"], "hello");
        assert_eq!(body["model_id"], "eleven_turbo_v2_5");
    }
}
