use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub action: String,
    pub payload: serde_json::Value,
}

impl Request {
    pub fn new(action: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            action: action.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Response {
    pub fn ok(action: &str, data: serde_json::Value) -> Self {
        Self {
            status: "ok".to_string(),
            message: action.to_string(),
            data: Some(data),
        }
    }

    pub fn ok_msg(message: impl Into<String>) -> Self {
        Self {
            status: "ok".to_string(),
            message: message.into(),
            data: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            status: "error".to_string(),
            message: message.into(),
            data: None,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == "ok"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_request_response() {
        let req = Request::new("LIST", serde_json::Value::Null);
        let s = serde_json::to_string(&req).unwrap();
        let back: Request = serde_json::from_str(&s).unwrap();
        assert_eq!(back.action, "LIST");

        let ok = Response::ok("LIST", serde_json::json!([1, 2]));
        assert!(ok.is_ok());
        let err = Response::err("boom");
        assert!(!err.is_ok());
        // NDJSON: sin salto interno.
        assert!(!serde_json::to_string(&ok).unwrap().contains('\n'));
    }
}
