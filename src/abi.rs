use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Request {
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            body: body.into(),
        }
    }
}

impl Response {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_through_json() {
        let request = Request::new("GET", "/hello", "");

        let json = serde_json::to_string(&request).unwrap();
        let decoded: Request = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, request);
    }

    #[test]
    fn response_round_trips_through_json() {
        let response = Response::new(200, r#"{"message":"hello"}"#);

        let json = serde_json::to_string(&response).unwrap();
        let decoded: Response = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, response);
    }
}
