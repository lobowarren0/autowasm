#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    Network,
    Filesystem,
    Environment,
    Database,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityPolicy {
    allowed: Vec<Capability>,
}

impl CapabilityPolicy {
    pub fn deny_all() -> Self {
        Self::allowing(std::iter::empty())
    }

    pub fn allowing(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            allowed: capabilities.into_iter().collect(),
        }
    }

    pub fn allows(&self, capability: &Capability) -> bool {
        self.allowed.iter().any(|allowed| allowed == capability)
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Capability::Network => "network",
            Capability::Filesystem => "filesystem",
            Capability::Environment => "environment",
            Capability::Database => "database",
        };

        write!(formatter, "{name}")
    }
}

pub fn detect(handler: &str) -> Vec<Capability> {
    let mut capabilities = Vec::new();

    if handler.contains("fetch(") {
        capabilities.push(Capability::Network);
    }

    if handler.contains("fs.") || handler.contains("readFile") || handler.contains("writeFile") {
        capabilities.push(Capability::Filesystem);
    }

    if handler.contains("process.env") {
        capabilities.push(Capability::Environment);
    }

    if handler.contains("db.") || handler.contains("database") {
        capabilities.push(Capability::Database);
    }

    capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_network_access() {
        let capabilities = detect(
            r#"
            async () => {
                const response = await fetch("https://example.com");
                return response.json();
            }
            "#,
        );

        assert_eq!(capabilities, vec![Capability::Network]);
    }

    #[test]
    fn detects_multiple_capabilities() {
        let capabilities = detect(
            r#"
            async () => {
                const value = process.env.API_KEY;
                const data = await fetch("https://example.com");
                return fs.readFileSync("data.json");
            }
            "#,
        );

        assert_eq!(
            capabilities,
            vec![
                Capability::Network,
                Capability::Filesystem,
                Capability::Environment,
            ]
        );
    }

    #[test]
    fn detects_no_capabilities() {
        let capabilities = detect(
            r#"
            () => {
                return "hello";
            }
            "#,
        );

        assert!(capabilities.is_empty());
    }

    #[test]
    fn denies_capabilities_by_default() {
        assert!(!CapabilityPolicy::deny_all().allows(&Capability::Network));
    }

    #[test]
    fn allows_configured_capabilities() {
        let policy = CapabilityPolicy::allowing([Capability::Network]);

        assert!(policy.allows(&Capability::Network));
        assert!(!policy.allows(&Capability::Filesystem));
    }
}

#[test]
fn detects_fetch_network_capability() {
    let handler = r#"
            async (c) => {
                const response = await fetch("https://example.com");
                return c.json(await response.json());
            }
        "#;

    let capabilities = detect(handler);

    assert_eq!(capabilities, vec![Capability::Network]);
}
