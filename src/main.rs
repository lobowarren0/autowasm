use std::env;
use std::path::Path;

mod analyzer;
mod detector;
mod framework;

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
        let source_path = repository.join("src").join("index.ts");

        if source_path.is_file() {
            match analyzer::discover_routes(&source_path) {
                Ok(routes) => {
                    println!();
                    println!("Routes:");

                    for route in routes {
                        println!("  {} {}", route.method, route.path);
                    }
                }
                Err(error) => {
                    eprintln!("Route discovery error: {error}");
                    std::process::exit(1);
                }
            }
        }
    }
}
