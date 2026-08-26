use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Language {
    JavaScript,
    TypeScript,
    Rust,
    Go,
    Unknown,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Rust => "Rust",
            Language::Go => "Go",
            Language::Unknown => "Unknown",
        };

        write!(f, "{name}")
    }
}

#[derive(Debug)]
pub struct DetectionResult {
    pub language: Language,
    pub confidence: f32,
    pub evidence: Vec<PathBuf>,
}

pub fn detect(repository: &Path) -> io::Result<DetectionResult> {
    if !repository.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "repository path does not exist",
        ));
    }

    if !repository.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path is not a directory",
        ));
    }

    let mut evidence = Vec::new();

    check_file(repository, "tsconfig.json", &mut evidence);
    check_file(repository, "package.json", &mut evidence);
    check_file(repository, "Cargo.toml", &mut evidence);
    check_file(repository, "go.mod", &mut evidence);

    let language = if contains_file(&evidence, "tsconfig.json") {
        Language::TypeScript
    } else if contains_file(&evidence, "package.json") {
        Language::JavaScript
    } else if contains_file(&evidence, "Cargo.toml") {
        Language::Rust
    } else if contains_file(&evidence, "go.mod") {
        Language::Go
    } else {
        Language::Unknown
    };

    let confidence = if language == Language::Unknown {
        0.0
    } else {
        1.0
    };

    Ok(DetectionResult {
        language,
        confidence,
        evidence,
    })
}

fn check_file(repository: &Path, filename: &str, evidence: &mut Vec<PathBuf>) {
    let path = repository.join(filename);

    if path.is_file() {
        evidence.push(path);
    }
}

fn contains_file(paths: &[PathBuf], filename: &str) -> bool {
    paths.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == filename)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestRepository {
        path: PathBuf,
    }

    impl TestRepository {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();

            let path = std::env::temp_dir().join(format!("autowasm-test-{timestamp}"));

            fs::create_dir_all(&path).unwrap();

            Self { path }
        }

        fn add_file(&self, filename: &str) {
            fs::write(self.path.join(filename), "").unwrap();
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn detects_rust() {
        let repo = TestRepository::new();
        repo.add_file("Cargo.toml");

        let result = detect(&repo.path).unwrap();

        assert_eq!(result.language, Language::Rust);
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn detects_javascript() {
        let repo = TestRepository::new();
        repo.add_file("package.json");

        let result = detect(&repo.path).unwrap();

        assert_eq!(result.language, Language::JavaScript);
    }

    #[test]
    fn detects_typescript() {
        let repo = TestRepository::new();
        repo.add_file("package.json");
        repo.add_file("tsconfig.json");

        let result = detect(&repo.path).unwrap();

        assert_eq!(result.language, Language::TypeScript);
    }

    #[test]
    fn detects_go() {
        let repo = TestRepository::new();
        repo.add_file("go.mod");

        let result = detect(&repo.path).unwrap();

        assert_eq!(result.language, Language::Go);
    }

    #[test]
    fn detects_unknown_repository() {
        let repo = TestRepository::new();

        let result = detect(&repo.path).unwrap();

        assert_eq!(result.language, Language::Unknown);
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn rejects_nonexistent_repository() {
        let path = std::env::temp_dir().join("autowasm-this-does-not-exist");

        let result = detect(&path);

        assert!(result.is_err());
    }
}
