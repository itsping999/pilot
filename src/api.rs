use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::Stream;
use log::info;
use std::convert::Infallible;

use crate::contracts::{
    CreateDocumentRequest, CreateDocumentResponse, DeleteDocumentResponse, HealthDetailResponse,
    HealthResponse, ListChunksResponse, ListDocumentsResponse, ListQueryHistoryResponse,
    QueryHistoryParams, QueryRequest, UpdateDocumentRequest,
};
use crate::http::{ApiError, ApiResponse};
use crate::ingestion::document_title_from_text;
use crate::middleware::{
    RateLimiter, cors_middleware, rate_limit_middleware, request_id_middleware,
};
use crate::models::{Document, RagResponse, default_access_scope, normalize_access_scope};
use crate::rag::RagService;
use crate::ui;

#[derive(Clone)]
pub struct AppState {
    rag: Arc<RagService>,
    api_token: Option<Arc<str>>,
}

pub fn build_router(
    rag: Arc<RagService>,
    ui_enabled: bool,
    request_body_limit_bytes: usize,
    api_token: Option<String>,
    requests_per_minute: u32,
) -> Router {
    let rate_limiter = Arc::new(RateLimiter::new(requests_per_minute));
    let state = AppState {
        rag,
        api_token: api_token.map(Arc::from),
    };
    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/health/detail", get(health_detail))
        .route("/documents", get(list_documents).post(create_document))
        .route("/documents/upload", post(upload_document))
        .route(
            "/documents/{id}",
            get(get_document)
                .put(update_document)
                .delete(delete_document),
        )
        .route("/documents/{id}/chunks", get(list_document_chunks))
        .route("/rag/query", post(query_rag))
        .route("/rag/query/stream", post(query_rag_stream))
        .route("/rag/history", get(list_query_history))
        .layer(DefaultBodyLimit::max(request_body_limit_bytes));

    let app = if ui_enabled {
        info!("UI enabled: serving at /, /upload, and /qa");
        api_routes
            .merge(Router::new().route("/", get(ui::serve_upload)))
            .merge(Router::new().route("/upload", get(ui::serve_upload)))
            .merge(Router::new().route("/qa", get(ui::serve_qa)))
            .merge(Router::new().route("/static/app.css", get(ui::serve_css)))
            .merge(Router::new().route("/static/app.js", get(ui::serve_js)))
            .with_state(state)
    } else {
        info!("UI disabled");
        api_routes.with_state(state)
    };

    app.layer(axum::middleware::from_fn(cors_middleware))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .layer(axum::middleware::from_fn_with_state(
            rate_limiter,
            rate_limit_middleware,
        ))
}

type ApiResult<T> = Result<Json<ApiResponse<T>>, ApiError>;

async fn health() -> Json<ApiResponse<HealthResponse>> {
    Json(ApiResponse::ok(HealthResponse {
        status: "ok".to_string(),
    }))
}

async fn health_detail(State(state): State<AppState>) -> Json<ApiResponse<HealthDetailResponse>> {
    let documents = state
        .rag
        .list_documents()
        .map(|docs| docs.len())
        .unwrap_or(0);
    let embedding_model = state.rag.embedding_model_name().to_string();
    let generator_type = state.rag.generator_type().to_string();

    Json(ApiResponse::ok(HealthDetailResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        documents,
        embedding_model,
        generator: generator_type,
    }))
}

async fn create_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateDocumentRequest>,
) -> ApiResult<CreateDocumentResponse> {
    require_auth(&state, &headers)?;
    let title = clean_required(request.title, "title")?;
    let text = clean_required(request.text, "text")?;
    let source = request
        .source
        .and_then(clean_optional)
        .unwrap_or_else(|| "local".to_string());
    let access_scope = request
        .access_scope
        .and_then(|scope| normalize_access_scope(&scope))
        .unwrap_or_else(default_access_scope);
    let id = request
        .id
        .and_then(clean_optional)
        .unwrap_or_else(|| format!("doc-{}", current_millis()));
    let document = Document {
        id: id.clone(),
        title,
        source,
        access_scope,
        text,
    };

    let rag = state.rag.clone();
    let chunks = run_blocking(move || rag.add_document(document)).await?;
    info!("document indexed: id={} chunks={}", id, chunks);

    Ok(Json(ApiResponse::ok(CreateDocumentResponse {
        id,
        chunks,
        status: "indexed".to_string(),
    })))
}

async fn update_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> ApiResult<CreateDocumentResponse> {
    require_auth(&state, &headers)?;

    let existing = {
        let rag = state.rag.clone();
        let id_for_lookup = id.clone();
        run_blocking(move || rag.get_document(&id_for_lookup)).await?
    };
    if existing.is_none() {
        return Err(ApiError::not_found("document not found"));
    }

    let existing = existing.unwrap();
    let title = request
        .title
        .and_then(clean_optional)
        .unwrap_or(existing.title);
    let text = request
        .text
        .and_then(clean_optional)
        .unwrap_or(existing.text);
    let source = request
        .source
        .and_then(clean_optional)
        .unwrap_or(existing.source);
    let access_scope = request
        .access_scope
        .and_then(|scope| normalize_access_scope(&scope))
        .unwrap_or(existing.access_scope);

    let document = Document {
        id: id.clone(),
        title,
        source,
        access_scope,
        text,
    };

    let rag = state.rag.clone();
    let chunks = run_blocking(move || rag.add_document(document)).await?;
    info!("document updated: id={} chunks={}", id, chunks);

    Ok(Json(ApiResponse::ok(CreateDocumentResponse {
        id,
        chunks,
        status: "updated".to_string(),
    })))
}

async fn upload_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<CreateDocumentResponse> {
    require_auth(&state, &headers)?;

    let mut id = None;
    let mut title = None;
    let mut source = None;
    let mut access_scope = None;
    let mut text = None;
    let mut filename = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request(format!("invalid multipart body: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        let field_filename = field.file_name().map(ToString::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|error| ApiError::bad_request(format!("invalid multipart field: {error}")))?;

        match name.as_str() {
            "id" => id = clean_optional(String::from_utf8_lossy(&bytes).to_string()),
            "title" => title = clean_optional(String::from_utf8_lossy(&bytes).to_string()),
            "source" => source = clean_optional(String::from_utf8_lossy(&bytes).to_string()),
            "access_scope" => {
                access_scope = normalize_access_scope(&String::from_utf8_lossy(&bytes))
            }
            "file" => {
                filename = field_filename;
                text = Some(
                    String::from_utf8(bytes.to_vec())
                        .map_err(|_| ApiError::bad_request("uploaded file must be UTF-8 text"))?,
                );
            }
            _ => {}
        }
    }

    let text = clean_required(text.unwrap_or_default(), "file")?;
    let source = source
        .or_else(|| filename.clone())
        .unwrap_or_else(|| "upload".to_string());
    let title = title
        .or_else(|| document_title_from_text(&text))
        .or_else(|| filename.as_deref().and_then(title_from_filename))
        .unwrap_or_else(|| source.clone());
    let access_scope = access_scope.unwrap_or_else(default_access_scope);
    let id = id.unwrap_or_else(|| format!("doc-{}", current_millis()));

    let document = Document {
        id: id.clone(),
        title,
        source,
        access_scope,
        text,
    };

    let rag = state.rag.clone();
    let chunks = run_blocking(move || rag.add_document(document)).await?;
    info!("document uploaded and indexed: id={} chunks={}", id, chunks);

    Ok(Json(ApiResponse::ok(CreateDocumentResponse {
        id,
        chunks,
        status: "indexed".to_string(),
    })))
}

async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<ListDocumentsResponse> {
    require_auth(&state, &headers)?;
    let rag = state.rag.clone();
    let documents = run_blocking(move || rag.list_documents()).await?;
    Ok(Json(ApiResponse::ok(ListDocumentsResponse { documents })))
}

async fn get_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Document> {
    require_auth(&state, &headers)?;
    let rag = state.rag.clone();
    run_blocking(move || rag.get_document(&id))
        .await?
        .map(ApiResponse::ok)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("document not found"))
}

async fn list_document_chunks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<ListChunksResponse> {
    require_auth(&state, &headers)?;
    let rag = state.rag.clone();
    let id_for_lookup = id.clone();
    if run_blocking(move || rag.get_document(&id_for_lookup))
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("document not found"));
    }

    let rag = state.rag.clone();
    let chunks = run_blocking(move || rag.list_document_chunks(&id)).await?;
    Ok(Json(ApiResponse::ok(ListChunksResponse { chunks })))
}

async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<DeleteDocumentResponse> {
    require_auth(&state, &headers)?;
    let rag = state.rag.clone();
    let id_for_delete = id.clone();
    if !run_blocking(move || rag.delete_document(&id_for_delete)).await? {
        return Err(ApiError::not_found("document not found"));
    }

    info!("document deleted: id={}", id);
    Ok(Json(ApiResponse::ok(DeleteDocumentResponse {
        id,
        status: "deleted".to_string(),
    })))
}

async fn query_rag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<QueryRequest>,
) -> ApiResult<RagResponse> {
    require_auth(&state, &headers)?;
    let question = clean_required(request.question, "question")?;
    let top_k = request.top_k.unwrap_or(5).clamp(1, 20);
    let mut access_scopes = request.access_scopes;
    if let Some(access_scope) = request.access_scope
        && let Some(access_scope) = normalize_access_scope(&access_scope)
    {
        access_scopes.push(access_scope);
    }
    let rag = state.rag.clone();
    let response =
        run_blocking(move || rag.answer_with_scopes(&question, top_k, &access_scopes)).await?;
    Ok(Json(ApiResponse::ok(response)))
}

async fn list_query_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<QueryHistoryParams>,
) -> ApiResult<ListQueryHistoryResponse> {
    require_auth(&state, &headers)?;
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let rag = state.rag.clone();
    let history = run_blocking(move || rag.list_query_history(limit)).await?;
    Ok(Json(ApiResponse::ok(ListQueryHistoryResponse { history })))
}

async fn query_rag_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<QueryRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    require_auth(&state, &headers)?;
    let question = clean_required(request.question, "question")?;
    let top_k = request.top_k.unwrap_or(5).clamp(1, 20);
    let mut access_scopes = request.access_scopes;
    if let Some(access_scope) = request.access_scope
        && let Some(access_scope) = normalize_access_scope(&access_scope)
    {
        access_scopes.push(access_scope);
    }

    let rag = state.rag.clone();
    let q = question.clone();
    let response = run_blocking(move || rag.answer_with_scopes(&q, top_k, &access_scopes)).await?;

    let answer = response.answer.clone();

    let stream = async_stream::stream! {
        let meta = serde_json::json!({
            "type": "metadata",
            "citations": response.citations,
            "confidence": response.confidence
        });
        yield Ok(Event::default().data(meta.to_string()));

        for ch in answer.chars() {
            let chunk = serde_json::json!({
                "type": "token",
                "content": ch.to_string()
            });
            yield Ok(Event::default().data(chunk.to_string()));
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }

        yield Ok(Event::default().data(r#"{"type":"done"}"#));
    };

    Ok(Sse::new(stream))
}

async fn run_blocking<T, E, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    E: Into<ApiError> + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ApiError::internal(format!("blocking task failed: {error}")))?
        .map_err(Into::into)
}

fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(expected) = &state.api_token else {
        return Ok(());
    };

    if bearer_token(headers).as_deref() == Some(expected.as_ref()) {
        return Ok(());
    }

    Err(ApiError::unauthorized())
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn clean_required(value: String, field: &str) -> Result<String, ApiError> {
    clean_optional(value).ok_or_else(|| ApiError::bad_request(format!("{field} is required")))
}

fn clean_optional(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn title_from_filename(filename: &str) -> Option<String> {
    let file_name = filename.rsplit('/').next().unwrap_or(filename);
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    clean_optional(stem.replace(['_', '-'], " "))
}

fn current_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}
