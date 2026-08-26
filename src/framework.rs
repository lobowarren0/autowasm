use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Framework {
    Hono,
    Unknown,
}

impl std::fmt::Display for Framework {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Framework::Hono => write!(f, "Hono"),
            Framework::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    dependencies: Option<std::collections::HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<std::collections::HashMap<String, String>>,
}

pub fn detect(repository: &Path) -> io::Result<Framework> {
    let package_json_path = repository.join("package.json");

    if !package_json_path.is_file() {
        return Ok(Framework::Unknown);
    }

    let contents = fs::read_to_string(package_json_path)?;

    let package: PackageJson = serde_json::from_str(&contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    if has_dependency(&package, "hono") {
        return Ok(Framework::Hono);
    }

    Ok(Framework::Unknown)
}

fn has_dependency(package: &PackageJson, name: &str) -> bool {
    package
        .dependencies
        .as_ref()
        .is_some_and(|dependencies| dependencies.contains_key(name))
        || package
            .dev_dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.contains_key(name))
}
