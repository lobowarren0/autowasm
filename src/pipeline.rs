use std::io;
use std::path::Path;

use crate::analyzer;
use crate::capability;
use crate::service::Service;
use crate::source;

pub fn analyze(repository: &Path) -> io::Result<Vec<Service>> {
    let source_files = source::discover_source_files(repository)?;

    let mut services = Vec::new();

    for source_file in source_files {
        let routes = analyzer::discover_routes(&source_file)?;

        for route in routes {
            let capabilities = capability::detect(&route.handler);

            let name = service_name(&route.method, &route.path);

            services.push(Service::new(
                name,
                route.method,
                route.path,
                route.handler,
                capabilities,
            ));
        }
    }

    Ok(services)
}

fn service_name(method: &str, path: &str) -> String {
    let normalized_path = path.trim_matches('/').replace('/', "-").replace(':', "");

    if normalized_path.is_empty() {
        format!("{}-root", method.to_lowercase())
    } else {
        format!("{}-{}", method.to_lowercase(), normalized_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_stable_service_name() {
        assert_eq!(service_name("GET", "/hello"), "get-hello");
    }

    #[test]
    fn creates_service_name_for_nested_path() {
        assert_eq!(service_name("POST", "/users/profile"), "post-users-profile");
    }

    #[test]
    fn creates_service_name_for_parameterized_path() {
        assert_eq!(service_name("DELETE", "/users/:id"), "delete-users-id");
    }

    #[test]
    fn creates_service_name_for_root_path() {
        assert_eq!(service_name("GET", "/"), "get-root");
    }
}
