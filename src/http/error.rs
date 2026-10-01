#![allow(dead_code)]

use axum::Json;
use axum::http::StatusCode;
use log::error;
use serde::Serialize;

use crate::rag::RagError;

// ---------------------------------------------------------------------------
// ErrorCode — stable, machine-readable identifiers for every error category
// ---------------------------------------------------------------------------

/// Centralized API error codes.
///
/// Each variant maps to a stable string identifier, an HTTP status code,
/// and a default human-readable message.  New variants should be added
/// here — not ad-hoc in handler code — so the full surface is visible in
/// one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    /// Request validation failed (missing fields, bad format, etc.).
    BadRequest,
    /// Bearer token missing or invalid.
    Unauthorized,
    /// Requested resource does not exist.
    NotFound,
    /// Request body exceeds the configured limit.
    PayloadTooLarge,
    /// SQLite persistence layer error.
    Database,
    /// Catch-all internal server error.
    Internal,
    /// Embedding provider (Ollama, etc.) returned an error.
    EmbeddingProvider,
    /// LLM generation provider returned an error.
    GenerationProvider,
    /// Request was well-formed but rejected by business logic.
    Conflict,
    /// Rate limit exceeded (reserved for future use).
    RateLimited,
}

impl ErrorCode {
    /// Stable snake_case identifier used in JSON error responses.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::NotFound => "not_found",
            Self::PayloadTooLarge => "payload_too_large",
            Self::Database => "database_error",
            Self::Internal => "internal_error",
            Self::EmbeddingProvider => "embedding_provider_error",
            Self::GenerationProvider => "generation_provider_error",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
        }
    }

    /// HTTP status code for this error category.
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Database => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::EmbeddingProvider => StatusCode::BAD_GATEWAY,
            Self::GenerationProvider => StatusCode::BAD_GATEWAY,
            Self::Conflict => StatusCode::CONFLICT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        }
    }

    /// Default English message shown when no override is provided.
    pub const fn default_message(self) -> &'static str {
        match self {
            Self::BadRequest => "bad request",
            Self::Unauthorized => "invalid or missing API token",
            Self::NotFound => "not found",
            Self::PayloadTooLarge => "request body too large",
            Self::Database => "database error",
            Self::Internal => "internal error",
            Self::EmbeddingProvider => "embedding provider error",
            Self::GenerationProvider => "generation provider error",
            Self::Conflict => "resource conflict",
            Self::RateLimited => "rate limit exceeded",
        }
    }
}

// ---------------------------------------------------------------------------
// ApiError — structured error that implements IntoResponse
// ---------------------------------------------------------------------------

/// Structured API error returned by all handler functions.
///
/// Contains an [`ErrorCode`] category and a human-readable message.
/// Implements [`axum::response::IntoResponse`] so handlers can use
/// `?` to convert errors into JSON responses automatically.
pub struct ApiError {
    code: ErrorCode,
    message: String,
}

impl ApiError {
    /// Create a new error with a custom message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 400 Bad Request with a custom message.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }

    /// 401 Unauthorized.
    pub fn unauthorized() -> Self {
        Self::new(
            ErrorCode::Unauthorized,
            ErrorCode::Unauthorized.default_message(),
        )
    }

    /// 404 Not Found with a custom message.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    /// 413 Payload Too Large.
    pub fn payload_too_large() -> Self {
        Self::new(
            ErrorCode::PayloadTooLarge,
            ErrorCode::PayloadTooLarge.default_message(),
        )
    }

    /// 409 Conflict with a custom message.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    /// 500 Internal Server Error with a custom message.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// The error code category.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// The human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

// ---------------------------------------------------------------------------
// From impls — wire common error types into ApiError
// ---------------------------------------------------------------------------

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        error!("database error: {}", error);
        Self::new(ErrorCode::Database, ErrorCode::Database.default_message())
    }
}

impl From<RagError> for ApiError {
    fn from(error: RagError) -> Self {
        match error {
            RagError::Database(error) => error.into(),
            RagError::Embed(error) => {
                error!("embedding error: {}", error);
                Self::new(
                    ErrorCode::EmbeddingProvider,
                    ErrorCode::EmbeddingProvider.default_message(),
                )
            }
            RagError::Generate(error) => {
                error!("generation error: {}", error);
                Self::new(
                    ErrorCode::GenerationProvider,
                    ErrorCode::GenerationProvider.default_message(),
                )
            }
        }
    }
}

impl From<crate::bot::BotError> for ApiError {
    fn from(error: crate::bot::BotError) -> Self {
        match error {
            crate::bot::BotError::Rag(err) => err.into(),
            crate::bot::BotError::Skill(err) => ApiError::bad_request(err.to_string()),
            crate::bot::BotError::Internal(msg) => ApiError::internal(msg),
        }
    }
}

// ---------------------------------------------------------------------------
// IntoResponse — serialize as { "code": "...", "message": "..." }
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(ErrorBody {
            code: self.code.as_str(),
            message: self.message,
        });
        (self.code.status(), body).into_response()
    }
}
