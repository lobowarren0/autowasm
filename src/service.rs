use crate::capability::Capability;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub method: String,
    pub path: String,
    pub handler: String,
    pub capabilities: Vec<Capability>,
}

impl Service {
    pub fn new(
        name: String,
        method: String,
        path: String,
        handler: String,
        capabilities: Vec<Capability>,
    ) -> Self {
        Self {
            name,
            method,
            path,
            handler,
            capabilities,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_service() {
        let service = Service::new(
            "hello".to_string(),
            "GET".to_string(),
            "/hello".to_string(),
            "(c) => c.text(\"hello\")".to_string(),
            vec![],
        );

        assert_eq!(service.name, "hello");
        assert_eq!(service.method, "GET");
        assert_eq!(service.path, "/hello");
        assert!(service.capabilities.is_empty());
    }
}
