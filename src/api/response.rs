use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

// ============================================================================
// JSend success envelope
// ============================================================================

#[derive(Debug, Serialize)]
pub struct JSend<T: Serialize> {
    pub data: T,
    pub status: &'static str,
}

impl<T: Serialize> JSend<T> {
    pub fn success(data: T) -> Json<JSend<T>> {
        Json(JSend {
            data,
            status: "success",
        })
    }
}

// ============================================================================
// JSend paginated envelope (for future use)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct JSendPaginated<T: Serialize> {
    pub data: PaginatedData<T>,
    pub status: &'static str,
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
            status: "success",
        })
    }
}

// ============================================================================
// JSend fail envelope (client errors, 4xx)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct JSendFail {
    pub data: FailData,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
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
                status: "fail",
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

#[derive(Debug, Serialize)]
pub struct JSendError {
    pub message: String,
    pub status: &'static str,
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
                status: "error",
            }),
        )
    }

    pub fn internal(message: impl Into<String>) -> (StatusCode, Json<JSendError>) {
        Self::response(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn bad_gateway(message: impl Into<String>) -> (StatusCode, Json<JSendError>) {
        Self::response(StatusCode::BAD_GATEWAY, message)
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
    pub fn not_found(message: impl Into<String>) -> Self {
        ApiError::Fail(StatusCode::NOT_FOUND, message.into())
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        ApiError::Fail(StatusCode::SERVICE_UNAVAILABLE, message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        ApiError::Error(StatusCode::INTERNAL_SERVER_ERROR, message.into())
    }

    pub fn bad_gateway(message: impl Into<String>) -> Self {
        ApiError::Error(StatusCode::BAD_GATEWAY, message.into())
    }

    pub fn from_status(status: StatusCode, message: impl Into<String>) -> Self {
        if status.is_client_error() {
            ApiError::Fail(status, message.into())
        } else {
            ApiError::Error(status, message.into())
        }
    }
}
