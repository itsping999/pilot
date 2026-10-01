use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::config::EmbeddingProviderConfig;

pub trait Embedder: Send + Sync {
    fn model_name(&self) -> &str;
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError>;
}

#[derive(Clone, Debug)]
pub struct HashEmbedder {
    dimensions: usize,
    model: String,
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self {
            dimensions: 256,
            model: "hash-256".to_string(),
        }
    }
}

impl Embedder for HashEmbedder {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut vector = vec![0.0; self.dimensions];

        for token in tokenize(text) {
            let index = stable_index(&token, self.dimensions);
            vector[index] += 1.0;
        }

        Ok(normalize(vector))
    }
}

pub struct OllamaEmbedder {
    config: EmbeddingProviderConfig,
    model_name: String,
    client: reqwest::blocking::Client,
}

impl OllamaEmbedder {
    pub fn new(config: EmbeddingProviderConfig) -> Self {
        let model_name = format!("ollama:{}", config.model);
        Self {
            config,
            model_name,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl Embedder for OllamaEmbedder {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let response = self
            .client()
            .post(self.config.endpoint_url())
            .json(&OllamaEmbedRequest {
                model: &self.config.model,
                input: text,
            })
            .send()
            .map_err(|error| EmbedError::Provider(error.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|error| EmbedError::Provider(error.to_string()))?;

        if !status.is_success() {
            return Err(EmbedError::Provider(format!(
                "ollama embedding request failed with status {status}: {body}"
            )));
        }

        let response: OllamaEmbedResponse =
            serde_json::from_str(&body).map_err(|error| EmbedError::Provider(error.to_string()))?;

        if let Some(error) = response.error.filter(|error| !error.trim().is_empty()) {
            return Err(EmbedError::Provider(error));
        }

        let vector = response
            .embeddings
            .and_then(|mut embeddings| embeddings.drain(..).next())
            .or(response.embedding)
            .filter(|vector| !vector.is_empty())
            .ok_or_else(|| EmbedError::Provider("empty Ollama embedding response".to_string()))?;

        Ok(normalize(vector))
    }
}

impl OllamaEmbedder {
    fn client(&self) -> &reqwest::blocking::Client {
        &self.client
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_lowercase())
        .collect()
}

fn stable_index(token: &str, dimensions: usize) -> usize {
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    (hasher.finish() as usize) % dimensions
}

fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length > 0.0 {
        for value in &mut vector {
            *value /= length;
        }
    }
    vector
}

#[derive(Debug)]
pub enum EmbedError {
    Provider(String),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(message) => write!(formatter, "embedding provider error: {message}"),
        }
    }
}

impl std::error::Error for EmbedError {}

#[derive(serde::Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(serde::Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Option<Vec<Vec<f32>>>,
    embedding: Option<Vec<f32>>,
    error: Option<String>,
}

pub fn cosine(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum()
}

pub fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&chunk| f32::from_le_bytes(chunk))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Embedder, HashEmbedder, OllamaEmbedResponse, cosine};

    #[test]
    fn related_text_has_positive_score() {
        let embedder = HashEmbedder::default();
        let left = embedder.embed("traditional rag pipeline").expect("embed");
        let right = embedder.embed("rag pipeline").expect("embed");

        assert!(cosine(&left, &right) > 0.0);
    }

    #[test]
    fn hash_embedder_reports_stable_model_name() {
        let embedder = HashEmbedder::default();

        assert_eq!(embedder.model_name(), "hash-256");
    }

    #[test]
    fn ollama_embed_response_accepts_current_api_shape() {
        let response: OllamaEmbedResponse =
            serde_json::from_str(r#"{"model":"m","embeddings":[[0.1,0.2]]}"#).expect("parse");

        assert_eq!(response.embeddings.unwrap()[0], vec![0.1, 0.2]);
    }
}
