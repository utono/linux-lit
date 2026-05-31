//! Voyage AI embedding client for semantic echo search.
//!
//! Embeds a single query string via the Voyage API. The model and
//! dimensions must match what `scripts/build_embeddings.py` used to
//! populate the `passage_embeddings` table.

const MODEL: &str = "voyage-3-large";

#[derive(Debug)]
pub enum VoyageError {
    MissingApiKey,
    Timeout,
    ApiError(String),
}

impl std::fmt::Display for VoyageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoyageError::MissingApiKey => write!(f, "Set VOYAGE_API_KEY environment variable"),
            VoyageError::Timeout => write!(f, "Voyage request timed out"),
            VoyageError::ApiError(msg) => write!(f, "Voyage API error: {}", msg),
        }
    }
}

/// Embed a single query string. Returns the embedding vector.
/// Uses `input_type="query"` to match the document-side embeddings.
pub async fn embed_query(text: &str) -> Result<Vec<f32>, VoyageError> {
    let api_key = std::env::var("VOYAGE_API_KEY").map_err(|_| VoyageError::MissingApiKey)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| VoyageError::ApiError(e.to_string()))?;

    let body = serde_json::json!({
        "input": [text],
        "model": MODEL,
        "input_type": "query",
    });

    let response = client
        .post("https://api.voyageai.com/v1/embeddings")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                VoyageError::Timeout
            } else {
                VoyageError::ApiError(e.to_string())
            }
        })?;

    let status = response.status();
    let text_body = response
        .text()
        .await
        .map_err(|e| VoyageError::ApiError(e.to_string()))?;

    if !status.is_success() {
        return Err(VoyageError::ApiError(format!("HTTP {}: {}", status, text_body)));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text_body).map_err(|e| VoyageError::ApiError(e.to_string()))?;

    let embedding = json
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("embedding"))
        .and_then(|e| e.as_array())
        .ok_or_else(|| VoyageError::ApiError("No embedding in response".to_string()))?;

    let vec: Vec<f32> = embedding
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();

    if vec.is_empty() {
        return Err(VoyageError::ApiError("Empty embedding vector".to_string()));
    }

    Ok(vec)
}
