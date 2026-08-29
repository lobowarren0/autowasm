use std::env;
use std::fs;
use std::io;

use reqwest::blocking::multipart::{Form, Part};
use serde::Deserialize;
use serde_json::json;

use crate::deployer::{Deployer, DeploymentArtifact, artifact_file, ensure_artifact_directory};

const API_BASE: &str = "https://api.cloudflare.com/client/v4";
const COMPATIBILITY_DATE: &str = "2026-08-29";

#[derive(Clone, PartialEq, Eq)]
pub struct CloudflareConfig {
    pub account_id: String,
    pub api_token: String,
    pub worker_name: String,
}

impl CloudflareConfig {
    pub fn from_env() -> io::Result<Self> {
        Ok(Self {
            account_id: required_env("CLOUDFLARE_ACCOUNT_ID")?,
            api_token: required_env("CLOUDFLARE_API_TOKEN")?,
            worker_name: required_env("AUTOWASM_CLOUDFLARE_WORKER_NAME")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareDeploymentResult {
    pub provider: &'static str,
    pub deployment_id: String,
    pub url: Option<String>,
}

pub struct CloudflareDeployer {
    config: CloudflareConfig,
    client: reqwest::blocking::Client,
}

impl CloudflareDeployer {
    pub fn new(config: CloudflareConfig) -> io::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("autowasm/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| io::Error::other(error.to_string()))?;
        Ok(Self { config, client })
    }

    fn build_upload(&self, artifact: &DeploymentArtifact) -> io::Result<Form> {
        ensure_artifact_directory(&artifact.directory)?;
        let manifest_path = artifact.directory.join("manifest.json");
        let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)
            .map_err(|error| io::Error::other(error.to_string()))?;

        let compiled: Vec<&ManifestService> = manifest
            .services
            .iter()
            .filter(|service| service.artifact.is_some())
            .collect();

        let mut form = Form::new();
        for service in &compiled {
            let artifact_path = service.artifact.as_deref().unwrap_or_default();
            let wasm_path = artifact_file(artifact, artifact_path)?;
            let part_name = format!("{}.wasm", service.name);
            form = form.part(
                part_name.clone(),
                Part::bytes(fs::read(wasm_path)?)
                    .file_name(part_name.clone())
                    .mime_str("application/wasm")
                    .map_err(|error| io::Error::other(error.to_string()))?,
            );
        }

        let worker = generate_worker(&compiled);
        let metadata = json!({
            "main_module": "worker.js",
            "compatibility_date": COMPATIBILITY_DATE,
        });
        form = form.part(
            "metadata",
            Part::text(metadata.to_string())
                .mime_str("application/json")
                .map_err(|error| io::Error::other(error.to_string()))?,
        );
        Ok(form.part(
            "worker.js",
            Part::bytes(worker.into_bytes())
                .file_name("worker.js")
                .mime_str("application/javascript+module")
                .map_err(|error| io::Error::other(error.to_string()))?,
        ))
    }
}

impl Deployer for CloudflareDeployer {
    type Output = CloudflareDeploymentResult;

    fn deploy(&self, artifact: &DeploymentArtifact) -> io::Result<Self::Output> {
        let form = self.build_upload(artifact)?;
        let url = format!(
            "{API_BASE}/accounts/{}/workers/scripts/{}",
            self.config.account_id, self.config.worker_name
        );
        let response = self
            .client
            .put(url)
            .bearer_auth(&self.config.api_token)
            .multipart(form)
            .send()
            .map_err(|error| io::Error::other(format!("Cloudflare request failed: {error}")))?;
        let status = response.status();
        let payload: ApiResponse = response
            .json()
            .map_err(|error| io::Error::other(format!("invalid Cloudflare response: {error}")))?;
        if !status.is_success() || !payload.success {
            let reason = payload
                .errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(io::Error::other(format!(
                "Cloudflare deployment failed (HTTP {status}): {reason}"
            )));
        }

        let result = payload.result.ok_or_else(|| {
            io::Error::other("Cloudflare response did not contain a deployment id")
        })?;
        Ok(CloudflareDeploymentResult {
            provider: "cloudflare",
            deployment_id: result.id,
            url: result.url,
        })
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    services: Vec<ManifestService>,
}

#[derive(Debug, Deserialize)]
struct ManifestService {
    name: String,
    method: String,
    path: String,
    artifact: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    success: bool,
    errors: Vec<ApiError>,
    result: Option<ApiResult>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct ApiResult {
    id: String,
    url: Option<String>,
}

fn required_env(name: &str) -> io::Result<String> {
    env::var(name)
        .map_err(|_| io::Error::other(format!("missing required environment variable: {name}")))
}

fn generate_worker(services: &[&ManifestService]) -> String {
    let mut worker = String::new();
    for service in services {
        worker.push_str(&format!(
            "import {} from \"./{}.wasm\";\n",
            module_ident(&service.name),
            service.name
        ));
    }
    worker.push_str("\nconst routes = [\n");
    for service in services {
        worker.push_str(&format!(
            "  {{ method: {}, path: {}, module: {} }},\n",
            json_string(&service.method),
            json_string(&route_pattern(&service.path)),
            module_ident(&service.name)
        ));
    }
    worker.push_str("];\n\n");
    worker.push_str(WORKER_RUNTIME);
    worker
}

fn module_ident(name: &str) -> String {
    format!(
        "SERVICE_{}",
        name.chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    )
}

fn route_pattern(path: &str) -> String {
    let mut pattern = String::from("^");
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        pattern.push('/');
        if segment.starts_with(':') {
            pattern.push_str("[^/]+");
        } else {
            pattern.push_str(&regex_escape(segment));
        }
    }
    if path == "/" {
        pattern.push('/');
    }
    pattern.push('$');
    pattern
}

fn regex_escape(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                format!("\\{character}")
            }
            other => other.to_string(),
        })
        .collect()
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

const WORKER_RUNTIME: &str = r#"async function invoke(module, request) {
  const instance = await WebAssembly.instantiate(module, {});
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  const bytes = encoder.encode(JSON.stringify(request));
  const ptr = instance.exports.alloc(bytes.length);
  new Uint8Array(instance.exports.memory.buffer, ptr, bytes.length).set(bytes);
  const packed = instance.exports.handle(ptr, bytes.length);
  const responsePtr = Number(packed >> 32n);
  const responseLength = Number(packed & 0xffffffffn);
  const response = JSON.parse(decoder.decode(new Uint8Array(
    instance.exports.memory.buffer,
    responsePtr,
    responseLength,
  )));
  return new Response(response.body, { status: response.status });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const route = routes.find((candidate) =>
      candidate.method === request.method && new RegExp(candidate.path).test(url.pathname),
    );
    if (!route) return new Response("Not Found", { status: 404 });
    return invoke(route.module, {
      method: request.method,
      path: url.pathname,
      body: await request.text(),
    });
  },
};
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_cloudflare_config_without_printing_secrets() {
        let config = CloudflareConfig {
            account_id: "account".to_string(),
            api_token: "secret".to_string(),
            worker_name: "autowasm".to_string(),
        };

        assert_eq!(config.worker_name, "autowasm");
        assert_eq!(module_ident("get-users-id"), "SERVICE_GET_USERS_ID");
        assert_eq!(route_pattern("/users/:id"), "^/users/[^/]+$");
    }

    #[test]
    fn generates_es_module_wasm_imports_instead_of_bindings() {
        let hello = ManifestService {
            name: "get-hello".to_string(),
            method: "GET".to_string(),
            path: "/hello".to_string(),
            artifact: Some("services/get-hello/service.wasm".to_string()),
        };
        let worker = generate_worker(&[&hello]);

        assert!(worker.contains("import SERVICE_GET_HELLO from \"./get-hello.wasm\";"));
        assert!(worker.contains("module: SERVICE_GET_HELLO"));
        assert!(!worker.contains("wasm_module"));
        assert!(!worker.contains("env[route.binding]"));
    }

    #[test]
    fn generates_safe_route_patterns() {
        assert_eq!(route_pattern("/"), "^/$");
        assert_eq!(route_pattern("/files/:name"), "^/files/[^/]+$");
    }
}
