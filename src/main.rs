use std::env;
use std::path::Path;

mod analyzer;
mod detector;
mod framework;
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

    if framework == framework::Framework::Hono {
        let source_files = match source::discover_source_files(repository) {
            Ok(files) => files,
            Err(error) => {
                eprintln!("Source discovery error: {error}");
                std::process::exit(1);
            }
        };

        let mut routes = Vec::new();

        for source_file in source_files {
            match analyzer::discover_routes(&source_file) {
                Ok(mut discovered) => routes.append(&mut discovered),
                Err(error) => {
                    eprintln!(
                        "Route discovery error in {}: {error}",
                        source_file.display()
                    );
                    std::process::exit(1);
                }
            }
        }

        println!();
        println!("Routes:");

        for route in routes {
            println!("  {} {}", route.method, route.path);
            println!("    Handler: {}", route.handler);
        }
    }
}
