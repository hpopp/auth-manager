use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::StatusCode;
use axum::Json;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

// ============================================================================
// JSend status enum
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JSendStatus {
    Error,
    Fail,
    Success,
}

// ============================================================================
// JSend success envelope
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct JSend<T: Serialize> {
    pub data: T,
    pub status: JSendStatus,
}

impl<T: Serialize> JSend<T> {
    pub fn success(data: T) -> Json<JSend<T>> {
        Json(JSend {
            data,
            status: JSendStatus::Success,
        })
    }
}

// ============================================================================
// JSend paginated envelope (for future use)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct JSendPaginated<T: Serialize> {
    pub data: PaginatedData<T>,
    pub status: JSendStatus,
}

#[derive(Debug, Serialize)]
pub struct PaginatedData<T: Serialize> {
    pub items: Vec<T>,
    pub pagination: Pagination,
}

#[derive(Debug, Serialize)]
pub struct Pagination {
    pub limit: u32,
    pub offset: u32,
    pub total: u64,
}

impl<T: Serialize> JSendPaginated<T> {
    pub fn success(items: Vec<T>, pagination: Pagination) -> Json<JSendPaginated<T>> {
        Json(JSendPaginated {
            data: PaginatedData { items, pagination },
            status: JSendStatus::Success,
        })
    }
}

// ============================================================================
// JSend fail envelope (client errors, 4xx)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct JSendFail {
    pub data: FailData,
    pub status: JSendStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FailData {
    pub message: String,
}

impl JSendFail {
    pub fn response(
        status_code: StatusCode,
        message: impl Into<String>,
    ) -> (StatusCode, Json<JSendFail>) {
        (
            status_code,
            Json(JSendFail {
                data: FailData {
                    message: message.into(),
                },
                status: JSendStatus::Fail,
            }),
        )
    }

    pub fn not_found(message: impl Into<String>) -> (StatusCode, Json<JSendFail>) {
        Self::response(StatusCode::NOT_FOUND, message)
    }

    pub fn unavailable(message: impl Into<String>) -> (StatusCode, Json<JSendFail>) {
        Self::response(StatusCode::SERVICE_UNAVAILABLE, message)
    }
}

// ============================================================================
// JSend error envelope (server errors, 5xx)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct JSendError {
    pub message: String,
    pub status: JSendStatus,
}

impl JSendError {
    pub fn response(
        status_code: StatusCode,
        message: impl Into<String>,
    ) -> (StatusCode, Json<JSendError>) {
        (
            status_code,
            Json(JSendError {
                message: message.into(),
                status: JSendStatus::Error,
            }),
        )
    }

    pub fn internal(message: impl Into<String>) -> (StatusCode, Json<JSendError>) {
        Self::response(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

// ============================================================================
// Unified error type for handlers
// ============================================================================

/// A JSend-compatible error that can be either a fail (4xx) or error (5xx).
/// Used as the error type in handler Result returns.
#[derive(Debug)]
pub enum ApiError {
    Fail(StatusCode, String),
    Error(StatusCode, String),
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Fail(code, msg) => {
                let (status, json) = JSendFail::response(code, msg);
                (status, json).into_response()
            }
            ApiError::Error(code, msg) => {
                let (status, json) = JSendError::response(code, msg);
                (status, json).into_response()
            }
        }
    }
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        ApiError::Fail(StatusCode::BAD_REQUEST, message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        ApiError::Fail(StatusCode::NOT_FOUND, message.into())
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        ApiError::Fail(StatusCode::SERVICE_UNAVAILABLE, message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        ApiError::Error(StatusCode::INTERNAL_SERVER_ERROR, message.into())
    }
}

// ============================================================================
// Custom extractors (reject with JSend-formatted ApiError)
// ============================================================================

/// Drop-in replacement for `axum::Json` that rejects with JSend errors.
pub struct AppJson<T>(pub T);

#[axum::async_trait]
impl<S, T> FromRequest<S> for AppJson<T>
where
    axum::Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => {
                let message = match rejection {
                    JsonRejection::JsonDataError(err) => {
                        format!("Invalid request body: {}", err.body_text())
                    }
                    JsonRejection::JsonSyntaxError(_) => "Malformed JSON in request body".into(),
                    JsonRejection::MissingJsonContentType(_) => {
                        "Missing Content-Type: application/json header".into()
                    }
                    _ => "Failed to read request body".into(),
                };
                Err(ApiError::bad_request(message))
            }
        }
    }
}

/// Drop-in replacement for `axum::extract::Query` that rejects with JSend errors.
/// Uses `serde_qs` for deserialization, which includes field names in error messages.
pub struct AppQuery<T>(pub T);

#[axum::async_trait]
impl<S, T> FromRequestParts<S> for AppQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, ApiError> {
        let query = parts.uri.query().unwrap_or_default();
        serde_qs::from_str(query)
            .map(AppQuery)
            .map_err(|e| ApiError::bad_request(friendly_query_error(&e.to_string())))
    }
}

/// Translate serde/serde_qs error messages into human-friendly descriptions.
fn friendly_query_error(raw: &str) -> String {
    let cleaned = raw
        .replace("u32", "non-negative integer")
        .replace("u64", "non-negative integer")
        .replace("i32", "integer")
        .replace("i64", "integer");

    format!("Invalid query parameter: {cleaned}")
}
