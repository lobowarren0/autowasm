use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::compiler;
use crate::pipeline;
use crate::service::Service;

const ARTIFACT_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct DeploymentManifest {
    pub version: u32,
    pub services: Vec<ManifestService>,
}

#[derive(Debug, Serialize)]
pub struct ManifestService {
    pub name: String,
    pub method: String,
    pub path: String,
    pub capabilities: Vec<String>,
    pub artifact: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DeploymentSummary {
    pub services: usize,
    pub compiled: usize,
    pub unsupported: usize,
    pub output: PathBuf,
}

pub fn deploy(repository: &Path) -> io::Result<DeploymentSummary> {
    let services = pipeline::analyze(repository)?;
    let output = repository.join(".autowasm");

    if output.exists() {
        fs::remove_dir_all(&output)?;
    }
    fs::create_dir_all(output.join("services"))?;

    let mut manifest_services = Vec::new();
    let mut compiled = 0;
    let mut unsupported = 0;

    for service in &services {
        let result = compile_and_write(service, &output);
        let (artifact, reason) = match result {
            Ok(artifact) => {
                compiled += 1;
                (Some(artifact), None)
            }
            Err(error) => {
                unsupported += 1;
                (None, Some(error.to_string()))
            }
        };

        manifest_services.push(ManifestService {
            name: service.name.clone(),
            method: service.method.clone(),
            path: service.path.clone(),
            capabilities: service
                .capabilities
                .iter()
                .map(ToString::to_string)
                .collect(),
            artifact,
            reason,
        });
    }

    let manifest = DeploymentManifest {
        version: ARTIFACT_VERSION,
        services: manifest_services,
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(output.join("manifest.json"), manifest_json)?;

    Ok(DeploymentSummary {
        services: services.len(),
        compiled,
        unsupported,
        output,
    })
}

fn compile_and_write(service: &Service, output: &Path) -> io::Result<String> {
    let service_directory = output.join("services").join(&service.name);
    fs::create_dir_all(&service_directory)?;
    let wasm = compiler::compile_service(service)?;
    fs::write(service_directory.join("service.wasm"), wasm)?;

    let metadata = serde_json::json!({
        "version": ARTIFACT_VERSION,
        "name": service.name,
        "method": service.method,
        "path": service.path,
        "capabilities": service.capabilities.iter().map(ToString::to_string).collect::<Vec<_>>(),
    });
    let metadata_json = serde_json::to_vec_pretty(&metadata)
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(service_directory.join("metadata.json"), metadata_json)?;

    Ok(format!("services/{}/service.wasm", service.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_supported_and_unsupported_services() {
        let repository = Path::new("fixtures/hono-app");
        let summary = deploy(repository).expect("fixture should deploy");

        assert_eq!(summary.services, 6);
        assert_eq!(summary.compiled, 5);
        assert_eq!(summary.unsupported, 1);
        assert!(
            summary
                .output
                .join("services/get-hello/service.wasm")
                .is_file()
        );
        assert!(
            summary
                .output
                .join("services/get-health/service.wasm")
                .is_file()
        );
        assert!(summary.output.join("manifest.json").is_file());

        let _ = fs::remove_dir_all(summary.output);
    }
}
