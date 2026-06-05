#![allow(dead_code)]

//! Centralized constants for default values, limits, and magic strings.
//!
//! Every numeric default, scope name, or repeated string literal used across
//! the codebase should live here so that changing a default is a one-line
//! edit rather than a hunt through multiple modules.

// ---------------------------------------------------------------------------
// Access scopes
// ---------------------------------------------------------------------------

/// Default access scope applied to documents when none is specified.
pub const DEFAULT_ACCESS_SCOPE: &str = "public";

// ---------------------------------------------------------------------------
// API defaults
// ---------------------------------------------------------------------------

/// Default `top_k` for RAG queries when the caller omits it.
pub const DEFAULT_TOP_K: usize = 5;

/// Minimum `top_k` value accepted by the API.
pub const MIN_TOP_K: usize = 1;

/// Maximum `top_k` value accepted by the API.
pub const MAX_TOP_K: usize = 20;

/// Default `limit` for paginated list endpoints.
pub const DEFAULT_LIST_LIMIT: usize = 20;

/// Maximum `limit` for paginated list endpoints.
pub const MAX_LIST_LIMIT: usize = 100;

/// Minimum `limit` for paginated list endpoints.
pub const MIN_LIST_LIMIT: usize = 1;

// ---------------------------------------------------------------------------
// Ingestion defaults
// ---------------------------------------------------------------------------

/// Default chunk size in characters.
pub const DEFAULT_CHUNK_SIZE: usize = 600;

/// Default overlap between adjacent chunks in characters.
pub const DEFAULT_CHUNK_OVERLAP: usize = 100;

// ---------------------------------------------------------------------------
// Request limits
// ---------------------------------------------------------------------------

/// Default request body limit in bytes (2 MiB).
pub const DEFAULT_REQUEST_BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;

// ---------------------------------------------------------------------------
// RAG pipeline limits
// ---------------------------------------------------------------------------

/// Maximum number of context chunks passed to the generator.
pub const MAX_CONTEXT_CHUNKS: usize = 20;

/// Neighbor window size for context expansion around retrieved chunks.
pub const NEIGHBOR_WINDOW: usize = 2;

// ---------------------------------------------------------------------------
// Document source
// ---------------------------------------------------------------------------

/// Default source label when none is provided via JSON API.
pub const DEFAULT_SOURCE_LOCAL: &str = "local";

/// Default source label for multipart file uploads.
pub const DEFAULT_SOURCE_UPLOAD: &str = "upload";

// ---------------------------------------------------------------------------
// Embedding
// ---------------------------------------------------------------------------

/// Deterministic hash embedder dimensionality.
pub const HASH_EMBEDDING_DIMENSIONS: usize = 256;

/// Model name reported by the deterministic hash embedder.
pub const HASH_EMBEDDING_MODEL: &str = "hash-256";

// ---------------------------------------------------------------------------
// SSE streaming
// ---------------------------------------------------------------------------

/// Delay between streamed tokens in milliseconds.
pub const SSE_TOKEN_DELAY_MS: u64 = 20;
