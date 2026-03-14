use axum::http::HeaderMap;

use crate::http_error::HttpError;

pub fn extract_user_id(headers: &HeaderMap, fallback_query: Option<i64>) -> Result<i64, HttpError> {
    if let Some(v) = headers.get("x-user-id") {
        let s = v
            .to_str()
            .map_err(|_| HttpError::bad_request("invalid x-user-id header"))?;
        let parsed: i64 = s
            .parse()
            .map_err(|_| HttpError::bad_request("invalid x-user-id header"))?;
        return Ok(parsed);
    }

    if let Some(user_id) = fallback_query {
        return Ok(user_id);
    }

    Err(HttpError::unauthorized(
        "missing x-user-id header (or user_id query param for testing)",
    ))
}
