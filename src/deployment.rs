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
    pub compiler_version: &'static str,
    pub runtime: &'static str,
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
    let staging = repository.join(".autowasm.staging");

    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(staging.join("services"))?;

    let mut manifest_services = Vec::new();
    let mut compiled = 0;
    let mut unsupported = 0;

    for service in &services {
        let result = compile_and_write(service, &staging);
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
        compiler_version: env!("CARGO_PKG_VERSION"),
        runtime: "wasmtime-48",
        services: manifest_services,
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(staging.join("manifest.json"), manifest_json)?;

    if output.exists() {
        if let Err(error) = fs::remove_dir_all(&output) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    }
    if let Err(error) = fs::rename(&staging, &output) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

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
        "compiler_version": env!("CARGO_PKG_VERSION"),
        "runtime": "wasmtime-48",
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

        assert_eq!(summary.services, 7);
        assert_eq!(summary.compiled, 6);
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

        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(summary.output.join("manifest.json")).expect("manifest should be readable"),
        )
        .expect("manifest should contain valid JSON");
        assert_eq!(manifest["version"], 1);
        assert_eq!(manifest["runtime"], "wasmtime-48");
        assert!(
            manifest["services"]
                .as_array()
                .unwrap()
                .iter()
                .any(|service| service["name"] == "get-external"
                    && service["artifact"].is_null()
                    && service["reason"].as_str().unwrap().contains("capabilities"))
        );

        let hello_response = crate::runtime::execute_request(
            &summary.output.join("services/get-hello/service.wasm"),
            &crate::abi::Request::new("GET", "/hello", ""),
        )
        .expect("packaged hello service should execute");
        assert_eq!(hello_response.status, 200);
        assert_eq!(hello_response.body, r#"{"message":"hello"}"#);

        let user_response = crate::runtime::execute_request(
            &summary
                .output
                .join("services/get-users-id-details/service.wasm"),
            &crate::abi::Request::new("GET", "/users/123/details", ""),
        )
        .expect("packaged parameter service should execute");
        assert_eq!(user_response.status, 200);
        assert_eq!(user_response.body, r#"{"id":"123"}"#);

        let _ = fs::remove_dir_all(summary.output);
    }

    #[test]
    fn packages_and_executes_javascript_hono_services() {
        let repository = Path::new("fixtures/hono-js");
        let services = pipeline::analyze(repository).expect("JavaScript fixture should analyze");

        assert_eq!(services.len(), 3);
        assert_eq!(services[0].name, "get-hello");
        assert_eq!(services[1].name, "get-health");
        assert_eq!(services[2].name, "get-users-id");

        let summary = deploy(repository).expect("JavaScript fixture should deploy");

        assert_eq!(summary.services, 3);
        assert_eq!(summary.compiled, 3);
        assert_eq!(summary.unsupported, 0);

        let hello_response = crate::runtime::execute_request(
            &summary.output.join("services/get-hello/service.wasm"),
            &crate::abi::Request::new("GET", "/hello", ""),
        )
        .expect("packaged JavaScript hello service should execute");
        assert_eq!(hello_response.status, 200);
        assert_eq!(hello_response.body, r#"{"message":"hello"}"#);

        let user_response = crate::runtime::execute_request(
            &summary.output.join("services/get-users-id/service.wasm"),
            &crate::abi::Request::new("GET", "/users/123", ""),
        )
        .expect("packaged JavaScript parameter service should execute");
        assert_eq!(user_response.status, 200);
        assert_eq!(user_response.body, r#"{"id":"123"}"#);

        let _ = fs::remove_dir_all(summary.output);
    }

    #[test]
    fn preserves_invalid_existing_output_on_replacement_failure() {
        let repository = tempfile::tempdir().expect("temporary repository should be created");
        fs::write(
            repository.path().join("index.js"),
            "const app = {}; app.get(\"/hello\", (c) => c.json({ ok: true }));",
        )
        .expect("source should be written");
        let output = repository.path().join(".autowasm");
        fs::write(&output, "existing output").expect("existing output should be written");

        assert!(deploy(repository.path()).is_err());
        assert!(output.is_file());
        assert!(!repository.path().join(".autowasm.staging").exists());
    }
}
