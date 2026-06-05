#![allow(dead_code)]

use serde::Serialize;

// ---------------------------------------------------------------------------
// ApiResponse<T> — uniform success envelope for all API endpoints
// ---------------------------------------------------------------------------

/// Shared success envelope.
///
/// Every successful API response wraps its payload in this structure:
///
/// ```json
/// { "code": "ok", "message": "ok", "data": { ... } }
/// ```
#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: &'static str,
    pub message: String,
    pub data: T,
}

impl<T: Serialize> ApiResponse<T> {
    /// Wrap `data` in the standard success envelope.
    pub fn ok(data: T) -> Self {
        Self {
            code: "ok",
            message: "ok".to_string(),
            data,
        }
    }

    /// Wrap `data` with a custom success message (e.g. for warnings).
    pub fn with_message(data: T, message: impl Into<String>) -> Self {
        Self {
            code: "ok",
            message: message.into(),
            data,
        }
    }
}

// ---------------------------------------------------------------------------
// PaginatedResponse<T> — paginated list envelope
// ---------------------------------------------------------------------------

/// Paginated list envelope.
///
/// Endpoints that return lists use this to communicate total counts and
/// pagination state alongside the items:
///
/// ```json
/// { "code": "ok", "message": "ok", "data": { "items": [...], "total": 42, "limit": 20, "offset": 0 } }
/// ```
#[derive(Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub code: &'static str,
    pub message: String,
    pub data: PaginatedData<T>,
}

#[derive(Serialize)]
pub struct PaginatedData<T: Serialize> {
    pub items: Vec<T>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

impl<T: Serialize> PaginatedResponse<T> {
    pub fn ok(items: Vec<T>, total: usize, limit: usize, offset: usize) -> Self {
        Self {
            code: "ok",
            message: "ok".to_string(),
            data: PaginatedData {
                items,
                total,
                limit,
                offset,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// StatusResponse — simple status-only responses
// ---------------------------------------------------------------------------

/// Simple status-only data payload for endpoints that just confirm an action.
#[derive(Serialize)]
pub struct StatusData {
    pub status: String,
}

impl StatusData {
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
        }
    }

    pub fn ok() -> Self {
        Self::new("ok")
    }
}
