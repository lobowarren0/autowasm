use std::io;
use std::path::{Path, PathBuf};

pub struct DeploymentArtifact {
    pub directory: PathBuf,
}

impl DeploymentArtifact {
    pub fn from_directory(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
}

pub trait Deployer {
    type Output;

    fn deploy(&self, artifact: &DeploymentArtifact) -> io::Result<Self::Output>;
}

pub fn artifact_file(artifact: &DeploymentArtifact, relative_path: &str) -> io::Result<PathBuf> {
    if !relative_path.starts_with("services/") || relative_path.contains("..") {
        return Err(io::Error::other("invalid deployment artifact path"));
    }

    let path = artifact.directory.join(relative_path);
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("deployment artifact is missing: {relative_path}"),
        ));
    }

    Ok(path)
}

pub fn ensure_artifact_directory(path: &Path) -> io::Result<()> {
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "deployment artifact directory does not exist",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn rejects_artifact_path_traversal() {
        let artifact = DeploymentArtifact::from_directory(".autowasm");

        assert!(artifact_file(&artifact, "services/../manifest.json").is_err());
    }

    #[test]
    fn accepts_service_artifact_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("services/get-hello");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("service.wasm"), b"wasm").unwrap();
        let artifact = DeploymentArtifact::from_directory(directory.path());

        assert!(artifact_file(&artifact, "services/get-hello/service.wasm").is_ok());
    }
}
