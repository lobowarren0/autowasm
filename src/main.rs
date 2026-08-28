use std::env;
use std::fs;
use std::path::Path;
use std::str::FromStr;

mod abi;
mod analyzer;
mod capability;
mod cloudflare;
mod compiler;
mod deployer;
mod deployment;
mod detector;
mod framework;
mod pipeline;
mod runtime;
mod service;
mod source;

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("analyze") => analyze_command(&args),
        Some("deploy") => deploy_command(&args),
        Some("build") => build_command(&args),
        Some("run") => run_command(&args),
        Some("invoke") => invoke_command(&args),
        _ => {
            eprintln!("Usage:");
            eprintln!("  autowasm analyze <repository-path>");
            eprintln!("  autowasm deploy <repository-path> [--provider cloudflare]");
            eprintln!("  autowasm deploy <repository-path> --provider cloudflare");
            eprintln!("  autowasm build <wat-path> <wasm-path>");
            eprintln!("  autowasm run <wasm-path>");
            eprintln!("  autowasm invoke <wasm-path> <method> <path> [body]");
            std::process::exit(1);
        }
    }
}

fn deploy_command(args: &[String]) {
    if args.len() < 3 {
        eprintln!("Usage: autowasm deploy <repository-path> [--allow-capability <name>]...");
        std::process::exit(1);
    }

    let repository = Path::new(&args[2]);
    let policy = match parse_capability_policy(args) {
        Ok(policy) => policy,
        Err(error) => {
            eprintln!("Deployment option error: {error}");
            std::process::exit(1);
        }
    };
    let provider = match parse_provider(args) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!("Deployment option error: {error}");
            std::process::exit(1);
        }
    };
    let detection = match detector::detect(repository) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };
    let framework = match framework::detect(repository) {
        Ok(framework) => framework,
        Err(error) => {
            eprintln!("Framework detection error: {error}");
            std::process::exit(1);
        }
    };

    println!("Analyzing repository: {}", repository.display());
    println!();
    println!("Language: {}", detection.language);
    println!("Framework: {framework}");

    let summary = match if args.len() == 3 {
        deployment::deploy(repository)
    } else {
        deployment::deploy_with_policy(repository, &policy)
    } {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("Deployment error: {error}");
            std::process::exit(1);
        }
    };

    println!();
    println!("Deployment:");
    println!("  Services: {}", summary.services);
    println!("  Compiled: {}", summary.compiled);
    println!("  Unsupported: {}", summary.unsupported);
    println!("  Output: {}", summary.output.display());
    println!();
    println!("Services:");
    for result in &summary.results {
        let status = if result.artifact.is_some() {
            "built"
        } else {
            "unsupported"
        };
        println!("  {} {}: {}", result.method, result.path, status);
        if let Some(reason) = &result.reason {
            println!("    Reason: {reason}");
        }
    }

    if provider.as_deref() == Some("cloudflare") {
        let config = match cloudflare::CloudflareConfig::from_env() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("Cloudflare configuration error: {error}");
                std::process::exit(1);
            }
        };
        let deployer = match cloudflare::CloudflareDeployer::new(config) {
            Ok(deployer) => deployer,
            Err(error) => {
                eprintln!("Cloudflare client error: {error}");
                std::process::exit(1);
            }
        };
        let artifact = deployer::DeploymentArtifact::from_directory(&summary.output);
        let result = match deployer::Deployer::deploy(&deployer, &artifact) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("Cloudflare deployment error: {error}");
                std::process::exit(1);
            }
        };
        println!();
        println!("Cloudflare deployment:");
        println!("  Deployment ID: {}", result.deployment_id);
        if let Some(url) = result.url {
            println!("  URL: {url}");
        } else {
            println!("  URL: unavailable from API response; configure a Worker route or subdomain");
        }
    }
}

fn parse_capability_policy(args: &[String]) -> Result<capability::CapabilityPolicy, String> {
    let mut allowed = Vec::new();
    let mut index = 3;

    while index < args.len() {
        if args.get(index).map(String::as_str) != Some("--allow-capability") {
            if args.get(index).map(String::as_str) == Some("--provider") {
                index += 2;
                continue;
            }
            return Err(format!("unknown option: {}", args[index]));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| "missing capability name".to_string())?;
        allowed.push(capability::Capability::from_str(value)?);
        index += 2;
    }

    Ok(capability::CapabilityPolicy::allowing(allowed))
}

fn parse_provider(args: &[String]) -> Result<Option<String>, String> {
    let mut provider = None;
    let mut index = 3;
    while index < args.len() {
        if args.get(index).map(String::as_str) == Some("--provider") {
            let value = args
                .get(index + 1)
                .ok_or_else(|| "missing provider name".to_string())?;
            if value != "cloudflare" {
                return Err(format!("unsupported provider: {value}"));
            }
            provider = Some(value.clone());
            index += 2;
        } else if args.get(index).map(String::as_str) == Some("--allow-capability") {
            index += 2;
        } else {
            return Err(format!("unknown option: {}", args[index]));
        }
    }
    Ok(provider)
}

fn analyze_command(args: &[String]) {
    if args.len() != 3 {
        eprintln!("Usage: autowasm analyze <repository-path>");
        std::process::exit(1);
    }

    let repository = Path::new(&args[2]);

    let detection = match detector::detect(repository) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };

    println!("Analyzing repository: {}", repository.display());
    println!();
    println!("Language: {}", detection.language);
    println!("Confidence: {:.0}%", detection.confidence * 100.0);
    println!("Evidence:");

    for file in &detection.evidence {
        println!("  - {}", file.display());
    }

    let framework = match framework::detect(repository) {
        Ok(framework) => framework,
        Err(error) => {
            eprintln!("Framework detection error: {error}");
            std::process::exit(1);
        }
    };

    println!();
    println!("Framework: {framework}");

    let services = match pipeline::analyze(repository) {
        Ok(services) => services,
        Err(error) => {
            eprintln!("Analysis error: {error}");
            std::process::exit(1);
        }
    };

    if services.is_empty() {
        println!();
        println!("Services: none");
        return;
    }

    println!();
    println!("Services:");

    for service in services {
        println!("  {} {}", service.method, service.path);
        println!("    Name: {}", service.name);
        println!("    Handler: {}", service.handler);

        if service.capabilities.is_empty() {
            println!("    Capabilities: none");
        } else {
            println!("    Capabilities:");

            for capability in &service.capabilities {
                println!("      - {capability}");
            }
        }
    }
}

fn build_command(args: &[String]) {
    if args.len() != 4 {
        eprintln!("Usage: autowasm build <wat-path> <wasm-path>");
        std::process::exit(1);
    }

    let wat_path = Path::new(&args[2]);
    let wasm_path = Path::new(&args[3]);

    let wat_source = match fs::read_to_string(wat_path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("Failed to read WAT source: {error}");
            std::process::exit(1);
        }
    };

    let wasm = match runtime::compile_wat(&wat_source) {
        Ok(wasm) => wasm,
        Err(error) => {
            eprintln!("WASM compilation error: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = fs::write(wasm_path, wasm) {
        eprintln!("Failed to write WASM module: {error}");
        std::process::exit(1);
    }

    println!("Built WASM module: {}", wasm_path.display());
}

fn run_command(args: &[String]) {
    if args.len() != 3 {
        eprintln!("Usage: autowasm run <wasm-path>");
        std::process::exit(1);
    }

    let module_path = Path::new(&args[2]);

    match runtime::execute_module(module_path) {
        Ok(result) => println!("WASM execution result: {result}"),
        Err(error) => {
            eprintln!("WASM execution error: {error}");
            std::process::exit(1);
        }
    }
}

fn invoke_command(args: &[String]) {
    if args.len() != 5 && args.len() != 6 {
        eprintln!("Usage: autowasm invoke <wasm-path> <method> <path> [body]");
        std::process::exit(1);
    }

    let module_path = Path::new(&args[2]);
    let method = &args[3];
    let path = &args[4];
    let body = args.get(5).map(String::as_str).unwrap_or("");

    let request = abi::Request::new(method, path, body);

    match runtime::execute_request(module_path, &request) {
        Ok(response) => {
            println!("Status: {}", response.status);
            println!("Body: {}", response.body);
        }
        Err(error) => {
            eprintln!("WASM invocation error: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repeatable_capability_flags() {
        let args = vec![
            "autowasm".to_string(),
            "deploy".to_string(),
            ".".to_string(),
            "--allow-capability".to_string(),
            "network".to_string(),
            "--allow-capability".to_string(),
            "filesystem".to_string(),
        ];

        let policy = parse_capability_policy(&args).unwrap();

        assert!(policy.allows(&capability::Capability::Network));
        assert!(policy.allows(&capability::Capability::Filesystem));
    }

    #[test]
    fn rejects_unknown_capability_flags() {
        let args = vec![
            "autowasm".to_string(),
            "deploy".to_string(),
            ".".to_string(),
            "--allow-capability".to_string(),
            "gpu".to_string(),
        ];

        assert!(parse_capability_policy(&args).is_err());
    }

    #[test]
    fn parses_cloudflare_provider() {
        let args = vec![
            "autowasm".to_string(),
            "deploy".to_string(),
            ".".to_string(),
            "--provider".to_string(),
            "cloudflare".to_string(),
        ];

        assert_eq!(
            parse_provider(&args).unwrap().as_deref(),
            Some("cloudflare")
        );
    }

    #[test]
    fn rejects_unknown_provider() {
        let args = vec![
            "autowasm".to_string(),
            "deploy".to_string(),
            ".".to_string(),
            "--provider".to_string(),
            "aws".to_string(),
        ];

        assert!(parse_provider(&args).is_err());
    }
}
