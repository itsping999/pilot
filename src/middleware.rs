use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

const X_REQUEST_ID: header::HeaderName = header::HeaderName::from_static("x-request-id");

// ---------------------------------------------------------------------------
// CORS middleware
// ---------------------------------------------------------------------------

pub async fn cors_middleware(request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("*"));

    if request.method() == Method::OPTIONS {
        let mut response = Response::default();
        *response.status_mut() = StatusCode::NO_CONTENT;
        insert_cors_headers(response.headers_mut(), &origin);
        return response;
    }

    let mut response = next.run(request).await;
    insert_cors_headers(response.headers_mut(), &origin);
    response
}

fn insert_cors_headers(headers: &mut axum::http::HeaderMap, origin: &HeaderValue) {
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Authorization, X-Request-Id"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("3600"),
    );
}

// ---------------------------------------------------------------------------
// Rate limiter — token-bucket per IP address
// ---------------------------------------------------------------------------

pub struct RateLimiter {
    state: Mutex<HashMap<String, TokenBucket>>,
    max_tokens: u32,
    refill_interval: Duration,
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            max_tokens: requests_per_minute,
            refill_interval: Duration::from_secs(60),
        }
    }

    fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());

        let bucket = state.entry(key.to_string()).or_insert_with(|| TokenBucket {
            tokens: self.max_tokens as f64,
            last_refill: now,
        });

        let elapsed = now.duration_since(bucket.last_refill);
        let refill =
            elapsed.as_secs_f64() / self.refill_interval.as_secs_f64() * self.max_tokens as f64;
        bucket.tokens = (bucket.tokens + refill).min(self.max_tokens as f64);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    let key = client_ip(&request);
    if !limiter.allow(&key) {
        let mut response = Response::default();
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        *response.body_mut() =
            axum::body::Body::from(r#"{"code":"rate_limited","message":"rate limit exceeded"}"#);
        return response;
    }
    next.run(request).await
}

fn client_ip(request: &Request) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            request
                .headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Request ID middleware
// ---------------------------------------------------------------------------

pub async fn request_id_middleware(mut request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(&X_REQUEST_ID)
        .cloned()
        .unwrap_or_else(|| {
            HeaderValue::from_str(&format!("req-{}", uuid_simple()))
                .unwrap_or_else(|_| HeaderValue::from_static("req-unknown"))
        });

    request
        .headers_mut()
        .insert(X_REQUEST_ID, request_id.clone());

    let mut response = next.run(request).await;
    response.headers_mut().insert(X_REQUEST_ID, request_id);
    response
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{t:032x}")
}

#[cfg(test)]
mod tests {
    use super::RateLimiter;

    #[test]
    fn rate_limiter_allows_initial_requests() {
        let limiter = RateLimiter::new(10);
        assert!(limiter.allow("127.0.0.1"));
    }

    #[test]
    fn rate_limiter_blocks_after_exhaustion() {
        let limiter = RateLimiter::new(2);
        assert!(limiter.allow("10.0.0.1"));
        assert!(limiter.allow("10.0.0.1"));
        assert!(!limiter.allow("10.0.0.1"));
    }

    #[test]
    fn rate_limiter_separates_clients() {
        let limiter = RateLimiter::new(1);
        assert!(limiter.allow("10.0.0.1"));
        assert!(limiter.allow("10.0.0.2"));
        assert!(!limiter.allow("10.0.0.1"));
    }
}
