use std::env;
use std::fs;
use std::path::Path;

mod abi;
mod analyzer;
mod capability;
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
        Some("build") => build_command(&args),
        Some("run") => run_command(&args),
        _ => {
            eprintln!("Usage:");
            eprintln!("  autowasm analyze <repository-path>");
            eprintln!("  autowasm build <wat-path> <wasm-path>");
            eprintln!("  autowasm run <wasm-path>");
            std::process::exit(1);
        }
    }
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
