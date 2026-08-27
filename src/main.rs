use std::env;
use std::path::Path;

mod analyzer;
mod capability;
mod detector;
mod framework;
mod pipeline;
mod service;
mod source;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 || args[1] != "analyze" {
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
