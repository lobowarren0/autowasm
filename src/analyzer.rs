use std::fs;
use std::io;
use std::path::Path;

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use tree_sitter_typescript::LANGUAGE_TYPESCRIPT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub method: String,
    pub path: String,
}

pub fn discover_routes(source_path: &Path) -> io::Result<Vec<Route>> {
    let source = fs::read_to_string(source_path)?;

    let mut parser = Parser::new();

    parser
        .set_language(&LANGUAGE_TYPESCRIPT.into())
        .map_err(|error| io::Error::other(error.to_string()))?;

    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| io::Error::other("failed to parse source"))?;

    let query = Query::new(
        &LANGUAGE_TYPESCRIPT.into(),
        r#"
        (call_expression
            function: (member_expression
                object: (identifier) @object
                property: (property_identifier) @method)
            arguments: (arguments
                (string) @path)
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

        for capture in match_.captures {
            let text = capture.node.utf8_text(source.as_bytes()).unwrap_or("");

            match capture.index {
                0 => object = Some(text),
                1 => method = Some(text),
                2 => path = Some(text.trim_matches('"').to_string()),
                _ => {}
            }
        }

        if object == Some("app") {
            if let (Some(method), Some(path)) = (method, path) {
                let method = method.to_uppercase();

                if matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
                    routes.push(Route { method, path });
                }
            }
        }
    }

    Ok(routes)
}
