use std::env;
use std::path::Path;

mod detector;
mod framework;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 || args[1] != "analyze" {
        eprintln!("Usage: autowasm analyze <repository-path>");
        std::process::exit(1);
    }

    let repository = Path::new(&args[2]);

    match detector::detect(repository) {
        Ok(result) => {
            println!("Analyzing repository: {}", repository.display());
            println!();
            println!("Language: {}", result.language);
            println!("Confidence: {:.0}%", result.confidence * 100.0);
            println!("Evidence:");

            for file in &result.evidence {
                println!("  - {}", file.display());
            }

            match framework::detect(repository) {
                Ok(framework) => {
                    println!();
                    println!("Framework: {framework}");
                }
                Err(error) => {
                    eprintln!("Framework detection error: {error}");
                    std::process::exit(1);
                }
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}
