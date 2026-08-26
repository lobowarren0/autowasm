use std::io;
use std::path::{Path, PathBuf};

const IGNORED_DIRECTORIES: &[&str] = &[
    ".git",
    "node_modules",
    "dist",
    "build",
    "target",
    ".next",
    "coverage",
];

const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];

pub fn discover_source_files(repository: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    visit_directory(repository, &mut files)?;

    files.sort();

    Ok(files)
}

fn visit_directory(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if should_ignore_directory(&path) {
                continue;
            }

            visit_directory(&path, files)?;
            continue;
        }

        if path.is_file() && is_source_file(&path) {
            files.push(path);
        }
    }

    Ok(())
}

fn should_ignore_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| IGNORED_DIRECTORIES.contains(&name))
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension))
}
