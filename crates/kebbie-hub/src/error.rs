use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kebbie_hub_api::ErrorBody;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    url: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str) -> ApiError {
        ApiError {
            status,
            code,
            url: None,
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn unauthorized() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized")
    }

    pub fn unknown_device() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unknown-device")
    }

    pub fn forbidden(code: &'static str) -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, code)
    }

    pub fn rate_limited() -> ApiError {
        ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate-limited")
    }

    pub fn not_found() -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not-found")
    }

    pub fn bad_request(code: &'static str) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, code)
    }

    pub fn internal() -> ApiError {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal")
    }

    pub fn conflict(code: &'static str) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, code)
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> ApiError {
        eprintln!("kebbie-hub: storage error: {error}");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "storage")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.code.to_string(),
                url: self.url,
            }),
        )
            .into_response()
    }
}
