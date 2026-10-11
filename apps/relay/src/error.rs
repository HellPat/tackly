//! How failures reach the client: a status code and a short message.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub struct ApiError(StatusCode, &'static str);

impl ApiError {
    pub const fn new(status: StatusCode, message: &'static str) -> Self {
        Self(status, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

pub type ApiResult<T> = Result<Json<T>, ApiError>;

/// A database failure: logged here, reported as a plain 500.
pub fn internal(error: sqlx::Error) -> ApiError {
    eprintln!("database error: {error}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
}

pub const fn bad_request(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, message)
}

pub const fn unauthorized(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, message)
}

pub const fn forbidden(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, message)
}

pub const fn conflict(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, message)
}

/// 410: an invitation that is expired, used, cancelled or never existed.
pub const fn gone(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::GONE, message)
}
