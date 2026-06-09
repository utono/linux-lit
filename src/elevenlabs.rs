use std::fmt;

/// Alice — "Clear, Engaging Educator". A `premade` voice that works on the
/// free ElevenLabs tier. Used as the default and as the 402 fallback when a
/// preferred (e.g. professional/library) voice is rejected with
/// `paid_plan_required`. Keep the id in ONE place so config and the fallback
/// path can't drift.
pub const ALICE_VOICE_ID: &str = "Xb7hH8MSUJpSbSDYk0k2";
pub const ALICE_MODEL_ID: &str = "eleven_turbo_v2_5";

/// The four custom Voice-Design narration voices (see
/// docs/guides/elevenlabs-v3-custom-voices.md "Saved voice IDs"). All render on
/// `eleven_v3` (the only model that reads inline /IPA/ and [audio tags]).
pub const A_OP_VOICE_ID: &str = "qIorOnPHyesnVMLvolyz"; // Will OP — male verse, OP
pub const B_VOICE_ID: &str = "jTudAEr52RK5998TOYLM"; // Will — male prose
pub const A_OP_F_VOICE_ID: &str = "AJEmTDfBuB294lokNL10"; // Willa OP — female verse, OP
pub const B_F_VOICE_ID: &str = "EKXvXWSM0PF7VaEykbP4"; // Willa — female prose
pub const OP_MODEL_ID: &str = "eleven_v3";

/// A character's gender, for gendered-voice selection. `Neutral` (deliberately
/// non-gendered) and `Unknown` (unresolved) both fall back to the male voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Male,
    Female,
    Neutral,
    Unknown,
}

impl Gender {
    /// Parse the stored TEXT value; anything unrecognized → `Unknown`.
    pub fn from_db(s: &str) -> Gender {
        match s {
            "male" => Gender::Male,
            "female" => Gender::Female,
            "neutral" => Gender::Neutral,
            _ => Gender::Unknown,
        }
    }
}

/// Pick (voice_id, model_id) for a `gender` reading a verse (`is_verse=true`,
/// the OP voice) or prose (`false`, the plain voice). Neutral/Unknown default to
/// the MALE set — never guess (per the design spec / guide).
pub fn voice_for(gender: Gender, is_verse: bool) -> (&'static str, &'static str) {
    let female = gender == Gender::Female;
    let id = match (female, is_verse) {
        (false, true) => A_OP_VOICE_ID,
        (false, false) => B_VOICE_ID,
        (true, true) => A_OP_F_VOICE_ID,
        (true, false) => B_F_VOICE_ID,
    };
    (id, OP_MODEL_ID)
}

#[derive(Debug)]
pub enum ElevenLabsError {
    MissingApiKey,
    Timeout,
    RateLimited,
    /// HTTP 402 / `paid_plan_required`: the voice needs a paid plan. Callers
    /// can detect this and fall back to a free voice (see [`ALICE_VOICE_ID`]).
    PaidPlanRequired,
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
            ElevenLabsError::PaidPlanRequired => {
                write!(f, "Voice requires a paid plan")
            }
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
    if status == reqwest::StatusCode::PAYMENT_REQUIRED {
        return Err(ElevenLabsError::PaidPlanRequired);
    }
    if !status.is_success() {
        let text = response.text().await.unwrap_or_else(|e| e.to_string());
        // Some plans surface the paywall with a non-402 status but a
        // `paid_plan_required` body — treat that as the same case.
        if text.contains("paid_plan_required") {
            return Err(ElevenLabsError::PaidPlanRequired);
        }
        return Err(ElevenLabsError::ApiError(format!("HTTP {}: {}", status, text)));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ElevenLabsError::ApiError(e.to_string()))?;
    Ok(bytes.to_vec())
}

/// A voice from the user's ElevenLabs account, for the voice picker.
#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub voice_id: String,
    pub name: String,
    /// ElevenLabs category: `premade` (free-tier safe), `professional`,
    /// `cloned`, `generated`, etc. Used to badge free vs paid in the picker.
    pub category: String,
}

impl VoiceInfo {
    /// `premade` voices work on the free tier; everything else typically
    /// returns `paid_plan_required` on a free plan.
    pub fn is_free_tier_safe(&self) -> bool {
        self.category == "premade"
    }
}

/// A short display label for a stored voice id, without a network call. Returns
/// "Alice" for the known fallback voice; otherwise the id itself (the settings
/// row shows the live name once the picker has been opened/confirmed).
pub fn voice_label_for_id(voice_id: &str) -> String {
    if voice_id == ALICE_VOICE_ID {
        "Alice".to_string()
    } else {
        voice_id.to_string()
    }
}

/// List the voices available on the account (`GET /v1/voices`). Key from
/// ELEVENLABS_API_KEY. Returns id/name/category for each voice.
pub async fn list_voices() -> Result<Vec<VoiceInfo>, ElevenLabsError> {
    let api_key =
        std::env::var("ELEVENLABS_API_KEY").map_err(|_| ElevenLabsError::MissingApiKey)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ElevenLabsError::ApiError(e.to_string()))?;

    let response = client
        .get("https://api.elevenlabs.io/v1/voices")
        .header("xi-api-key", &api_key)
        .header("accept", "application/json")
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
        let text = response.text().await.unwrap_or_else(|e| e.to_string());
        return Err(ElevenLabsError::ApiError(format!("HTTP {}: {}", status, text)));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| ElevenLabsError::ApiError(e.to_string()))?;
    Ok(parse_voices(&json))
}

/// Extract `VoiceInfo`s from a `/v1/voices` response body. Voices missing an
/// id are skipped; a missing name falls back to the id; a missing category to
/// "premade" (the safe assumption).
fn parse_voices(json: &serde_json::Value) -> Vec<VoiceInfo> {
    json.get("voices")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let voice_id = v.get("voice_id")?.as_str()?.to_string();
                    let name = v
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or(&voice_id)
                        .to_string();
                    let category = v
                        .get("category")
                        .and_then(|c| c.as_str())
                        .unwrap_or("premade")
                        .to_string();
                    Some(VoiceInfo {
                        voice_id,
                        name,
                        category,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_for_male_verse_is_aop() {
        assert_eq!(voice_for(Gender::Male, true), (A_OP_VOICE_ID, OP_MODEL_ID));
    }

    #[test]
    fn voice_for_female_prose_is_bf() {
        assert_eq!(voice_for(Gender::Female, false), (B_F_VOICE_ID, OP_MODEL_ID));
    }

    #[test]
    fn voice_for_neutral_and_unknown_default_to_male() {
        // neutral/unknown -> male set (never guess)
        assert_eq!(voice_for(Gender::Neutral, true), (A_OP_VOICE_ID, OP_MODEL_ID));
        assert_eq!(voice_for(Gender::Unknown, false), (B_VOICE_ID, OP_MODEL_ID));
    }

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
        assert_eq!(
            ElevenLabsError::PaidPlanRequired.to_string(),
            "Voice requires a paid plan"
        );
    }

    #[test]
    fn alice_is_free_tier_safe() {
        let alice = VoiceInfo {
            voice_id: ALICE_VOICE_ID.to_string(),
            name: "Alice".to_string(),
            category: "premade".to_string(),
        };
        assert!(alice.is_free_tier_safe());
        let pro = VoiceInfo {
            category: "professional".to_string(),
            ..alice
        };
        assert!(!pro.is_free_tier_safe());
    }

    #[test]
    fn parse_voices_extracts_id_name_category() {
        let json = serde_json::json!({
            "voices": [
                { "voice_id": "v1", "name": "Alice", "category": "premade" },
                { "voice_id": "v2", "name": "Will", "category": "professional" },
                { "name": "no id — skipped" },
                { "voice_id": "v3" }
            ]
        });
        let voices = parse_voices(&json);
        assert_eq!(voices.len(), 3);
        assert_eq!(voices[0].voice_id, "v1");
        assert_eq!(voices[0].name, "Alice");
        assert!(voices[0].is_free_tier_safe());
        assert!(!voices[1].is_free_tier_safe());
        // missing name falls back to id; missing category defaults premade
        assert_eq!(voices[2].voice_id, "v3");
        assert_eq!(voices[2].name, "v3");
        assert_eq!(voices[2].category, "premade");
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
