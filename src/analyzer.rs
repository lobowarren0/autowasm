use std::fs;
use std::io;
use std::path::Path;

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use tree_sitter_javascript::LANGUAGE as LANGUAGE_JAVASCRIPT;
use tree_sitter_typescript::LANGUAGE_TYPESCRIPT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub handler: String,
}

pub fn discover_routes(source_path: &Path) -> io::Result<Vec<Route>> {
    let source = fs::read_to_string(source_path)?;

    let mut parser = Parser::new();

    let language = if matches!(
        source_path
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("js") | Some("jsx")
    ) {
        LANGUAGE_JAVASCRIPT.into()
    } else {
        LANGUAGE_TYPESCRIPT.into()
    };

    parser
        .set_language(&language)
        .map_err(|error| io::Error::other(error.to_string()))?;

    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| io::Error::other("failed to parse source"))?;

    let query = Query::new(
        &language,
        r#"
        (call_expression
            function: (member_expression
                object: (identifier) @object
                property: (property_identifier) @method)
            arguments: (arguments
                (string) @path
                (_) @handler)
        )
        "#,
    )
    .map_err(|error| io::Error::other(error.to_string()))?;

    let mut cursor = QueryCursor::new();
    let mut routes = Vec::new();

    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    while let Some(match_) = matches.next() {
        let mut object = None;
        let mut method = None;
        let mut path = None;
        let mut handler = None;

        for capture in match_.captures {
            let text = capture.node.utf8_text(source.as_bytes()).unwrap_or("");

            match capture.index {
                0 => object = Some(text),
                1 => method = Some(text),
                2 => path = Some(text.trim_matches('"').to_string()),
                3 => handler = Some(text.to_string()),
                _ => {}
            }
        }

        if object == Some("app") {
            if let (Some(method), Some(path), Some(handler)) = (method, path, handler) {
                let method = method.to_uppercase();

                if matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
                    routes.push(Route {
                        method,
                        path,
                        handler,
                    });
                }
            }
        }
    }

    Ok(routes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestSource {
        path: std::path::PathBuf,
    }

    impl TestSource {
        fn new(source: &str) -> Self {
            Self::with_extension(source, "ts")
        }

        fn with_extension(source: &str, extension: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();

            let path =
                std::env::temp_dir().join(format!("autowasm-analyzer-{timestamp}.{extension}"));

            fs::write(&path, source).unwrap();

            Self { path }
        }
    }

    impl Drop for TestSource {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    #[test]
    fn discovers_get_route_and_handler() {
        let source = TestSource::new(
            r#"
            const app = new Hono();

            app.get("/hello", (c) => {
                return c.json({ message: "hello" });
            });
            "#,
        );

        let routes = discover_routes(&source.path).unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/hello");
        assert!(routes[0].handler.contains("c.json"));
    }

    #[test]
    fn discovers_multiple_routes_and_handlers() {
        let source = TestSource::new(
            r#"
            const app = new Hono();

            app.get("/users", (c) => {
                return c.json({ users: [] });
            });

            app.post("/users", (c) => {
                return c.json({ created: true });
            });

            app.delete("/users/:id", (c) => {
                return c.json({ deleted: true });
            });
            "#,
        );

        let routes = discover_routes(&source.path).unwrap();

        assert_eq!(routes.len(), 3);

        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/users");
        assert!(routes[0].handler.contains("users"));

        assert_eq!(routes[1].method, "POST");
        assert_eq!(routes[1].path, "/users");
        assert!(routes[1].handler.contains("created"));

        assert_eq!(routes[2].method, "DELETE");
        assert_eq!(routes[2].path, "/users/:id");
        assert!(routes[2].handler.contains("deleted"));
    }

    #[test]
    fn ignores_calls_without_handlers() {
        let source = TestSource::new(
            r#"
            const app = new Hono();

            app.get("/hello");
            app.listen(3000);
            "#,
        );

        let routes = discover_routes(&source.path).unwrap();

        assert!(routes.is_empty());
    }

    #[test]
    fn returns_error_for_missing_file() {
        let path = std::env::temp_dir().join("autowasm-missing-source.ts");

        let result = discover_routes(&path);

        assert!(result.is_err());
    }

    #[test]
    fn extracts_network_route_handler() {
        let source = TestSource::new(
            r#"
            const app = new Hono();

            app.get("/external", async (c) => {
                const response = await fetch("https://example.com");
                return c.json(await response.json());
            });
            "#,
        );

        let routes = discover_routes(&source.path).unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/external");
        assert!(routes[0].handler.contains("fetch"));
    }

    #[test]
    fn discovers_javascript_routes_and_parameters() {
        let source = TestSource::with_extension(
            r#"
                        import { Hono } from "hono";
                        const app = new Hono();

                        app.get("/hello", (c) => {
                            return c.json({ message: "hello" });
                        });
                        app.get("/health", (c) => {
                            return c.json({ status: "ok" });
                        });
                        app.post("/users", (c) => {
                            return c.json({ created: true });
                        });
                        app.delete("/users/:id", (c) => {
                            return c.json({ id: c.req.param("id") });
                        });
                        "#,
            "js",
        );

        let routes = discover_routes(&source.path).unwrap();

        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/hello");
        assert_eq!(routes[1].path, "/health");
        assert_eq!(routes[2].method, "POST");
        assert_eq!(routes[3].path, "/users/:id");
        assert!(routes[3].handler.contains("c.req.param"));
    }

    #[test]
    fn discovers_javascript_network_capability_handler() {
        let source = TestSource::with_extension(
            r#"
                        const app = new Hono();
                        app.get("/external", async (c) => {
                            const response = await fetch("https://example.com");
                            return c.json(await response.json());
                        });
                        "#,
            "js",
        );

        let routes = discover_routes(&source.path).unwrap();

        assert_eq!(routes.len(), 1);
        assert!(
            crate::capability::detect(&routes[0].handler)
                .contains(&crate::capability::Capability::Network)
        );
    }
}
