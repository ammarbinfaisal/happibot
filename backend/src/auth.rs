use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::http_error::HttpError;

// Telegram Mini App auth:
// https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app
//
// We accept either:
// - X-Telegram-Init-Data: the initData string from Telegram.WebApp.initData
// - X-User-Id: a dev-only fallback (useful for local testing without Telegram)
pub fn auth_user_id(headers: &HeaderMap, bot_token: Option<&str>) -> Result<i64, HttpError> {
    if let Some(init_data) = headers.get("x-telegram-init-data") {
        let init_data = init_data
            .to_str()
            .map_err(|_| HttpError::bad_request("invalid x-telegram-init-data header"))?;

        let bot_token = bot_token.ok_or_else(|| {
            HttpError::unauthorized("TELEGRAM_BOT_TOKEN not configured for initData auth")
        })?;

        let user_id = verify_init_data_and_get_user_id(init_data, bot_token)?;
        return Ok(user_id);
    }

    if let Some(v) = headers.get("x-user-id") {
        let s = v
            .to_str()
            .map_err(|_| HttpError::bad_request("invalid x-user-id header"))?;
        let parsed: i64 = s
            .parse()
            .map_err(|_| HttpError::bad_request("invalid x-user-id header"))?;
        return Ok(parsed);
    }

    Err(HttpError::unauthorized(
        "missing x-telegram-init-data or x-user-id",
    ))
}

fn verify_init_data_and_get_user_id(init_data: &str, bot_token: &str) -> Result<i64, HttpError> {
    // init_data is querystring like "query_id=...&user=...&auth_date=...&hash=..."
    // Values are percent-encoded; Telegram computes the hash on decoded values,
    // so we must decode before building data_check_string.
    let raw_pairs: Vec<(&str, &str)> = init_data
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .collect();

    let mut provided_hash: Option<&str> = None;
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (k, v) in &raw_pairs {
        if *k == "hash" {
            provided_hash = Some(*v);
        } else {
            let decoded_v = percent_decode(v)?;
            pairs.push((k.to_string(), decoded_v));
        }
    }

    let provided_hash =
        provided_hash.ok_or_else(|| HttpError::unauthorized("initData missing hash"))?;

    // data_check_string: sort by key and join "key=value" with "\n"
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let data_check_string = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");

    // secret_key = HMAC_SHA256("WebAppData", bot_token)
    let secret_key = {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"WebAppData")
            .map_err(|_| HttpError::bad_request("hmac init failed"))?;
        mac.update(bot_token.as_bytes());
        mac.finalize().into_bytes()
    };

    // computed_hash = HMAC_SHA256(secret_key, data_check_string) hex
    let computed_hash = {
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret_key)
            .map_err(|_| HttpError::bad_request("hmac init failed"))?;
        mac.update(data_check_string.as_bytes());
        let bytes = mac.finalize().into_bytes();
        hex::encode(bytes)
    };

    if !constant_time_eq(computed_hash.as_bytes(), provided_hash.as_bytes()) {
        return Err(HttpError::unauthorized("invalid initData hash"));
    }

    // Extract user.id from the "user" field (already decoded above).
    let user_json = pairs
        .iter()
        .find(|(k, _)| k == "user")
        .map(|(_, v)| v.as_str())
        .ok_or_else(|| HttpError::unauthorized("initData missing user field"))?;

    let user_val: serde_json::Value =
        serde_json::from_str(user_json).map_err(|_| HttpError::bad_request("bad user json"))?;

    let user_id = user_val
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| HttpError::bad_request("user.id missing in initData"))?;

    Ok(user_id)
}

fn percent_decode(input: &str) -> Result<String, HttpError> {
    // querystring form: '+' is space, %XX is byte.
    let input = input.replace('+', " ");
    let mut out = Vec::<u8>::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(HttpError::bad_request("bad percent-encoding"));
            }
            let hi = from_hex(bytes[i + 1])?;
            let lo = from_hex(bytes[i + 2])?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| HttpError::bad_request("bad utf-8 in initData"))
}

fn from_hex(b: u8) -> Result<u8, HttpError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(HttpError::bad_request("bad percent-encoding")),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut x = 0u8;
    for i in 0..a.len() {
        x |= a[i] ^ b[i];
    }
    x == 0
}
