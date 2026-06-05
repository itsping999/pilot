use serde::{Deserialize, Serialize};

use crate::models::{Chunk, DocumentSummary, QueryHistoryEntry};

#[derive(Deserialize)]
pub struct CreateDocumentRequest {
    pub id: Option<String>,
    pub title: String,
    pub text: String,
    pub source: Option<String>,
    pub access_scope: Option<String>,
}

#[derive(Serialize)]
pub struct CreateDocumentResponse {
    pub id: String,
    pub chunks: usize,
    pub status: String,
}

#[derive(Serialize)]
pub struct DeleteDocumentResponse {
    pub id: String,
    pub status: String,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Serialize)]
pub struct ListChunksResponse {
    pub chunks: Vec<Chunk>,
}

#[derive(Serialize)]
pub struct ListDocumentsResponse {
    pub documents: Vec<DocumentSummary>,
}

#[derive(Deserialize)]
pub struct QueryHistoryParams {
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct ListQueryHistoryResponse {
    pub history: Vec<QueryHistoryEntry>,
}

#[derive(Deserialize)]
pub struct QueryRequest {
    pub question: String,
    pub top_k: Option<usize>,
    pub access_scope: Option<String>,
    #[serde(default)]
    pub access_scopes: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateDocumentRequest {
    pub title: Option<String>,
    pub text: Option<String>,
    pub source: Option<String>,
    pub access_scope: Option<String>,
}

#[derive(Serialize)]
pub struct HealthDetailResponse {
    pub status: String,
    pub version: String,
    pub documents: usize,
    pub embedding_model: String,
    pub generator: String,
}
