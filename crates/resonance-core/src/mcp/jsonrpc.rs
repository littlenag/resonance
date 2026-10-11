//! JSON-RPC 2.0 framing over newline-delimited stdio.

use serde_json::{Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// One incoming message.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A request, which needs a response.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// A notification, which must not be answered.
    Notification { method: String, params: Value },
    /// A response to something we sent (unused: this server sends no requests).
    Response,
}

/// Parse one line. `Err` carries the error response to send.
pub fn parse(line: &str) -> Result<Incoming, Value> {
    let v: Value = serde_json::from_str(line)
        .map_err(|e| error(Value::Null, PARSE_ERROR, &format!("parse error: {e}")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| error(Value::Null, INVALID_REQUEST, "expected a JSON object"))?;
    let id = obj.get("id").cloned();
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(error(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "expected jsonrpc 2.0",
        ));
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        if obj.contains_key("result") || obj.contains_key("error") {
            return Ok(Incoming::Response);
        }
        return Err(error(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "missing method",
        ));
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    Ok(match id {
        Some(id) => Incoming::Request {
            id,
            method: method.to_string(),
            params,
        },
        None => Incoming::Notification {
            method: method.to_string(),
            params,
        },
    })
}

pub fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests_and_notifications() {
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap(),
            Incoming::Request {
                id: json!(1),
                method: "ping".into(),
                params: Value::Null
            }
        );
        assert!(matches!(
            parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap(),
            Incoming::Notification { .. }
        ));
        assert_eq!(
            parse("{not json").unwrap_err()["error"]["code"],
            PARSE_ERROR
        );
        assert_eq!(
            parse(r#"{"id":1,"method":"x"}"#).unwrap_err()["error"]["code"],
            INVALID_REQUEST
        );
    }
}
