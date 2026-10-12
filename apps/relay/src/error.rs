//! How failures reach the client: a status code and a short message, as
//! `{"error": "..."}`.

use tackly_wire::ApiErrorBody;
use worker::Response;

#[derive(Debug)]
pub struct ApiError(pub u16, pub &'static str);

pub type ApiResult = Result<Response, ApiError>;

impl ApiError {
    pub fn into_response(self) -> worker::Result<Response> {
        Ok(Response::from_json(&ApiErrorBody {
            error: self.1.to_owned(),
        })?
        .with_status(self.0))
    }
}

/// A failure of D1 or the runtime: logged here, reported as a plain 500.
pub fn internal(error: impl std::fmt::Display) -> ApiError {
    worker::console_error!("relay error: {error}");
    ApiError(500, "database error")
}

pub const fn bad_request(message: &'static str) -> ApiError {
    ApiError(400, message)
}

pub const fn unauthorized(message: &'static str) -> ApiError {
    ApiError(401, message)
}

pub const fn forbidden(message: &'static str) -> ApiError {
    ApiError(403, message)
}

pub const fn not_found(message: &'static str) -> ApiError {
    ApiError(404, message)
}

pub const fn conflict(message: &'static str) -> ApiError {
    ApiError(409, message)
}

/// 410: an invitation or request that is expired, used, cancelled or never
/// existed.
pub const fn gone(message: &'static str) -> ApiError {
    ApiError(410, message)
}

pub const fn too_many(message: &'static str) -> ApiError {
    ApiError(429, message)
}
