//! File abilities confined to the workspace directory.

use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::shell::Shell;
use crate::shell::ShellError;
use crate::text::head;

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("path must be relative and stay inside the workspace: {0}")]
    Escape(String),

    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    #[error("old text not found in {0}")]
    NotFound(String),

    #[error("old text occurs {count} times in {path}, make it unique")]
    Ambiguous { path: String, count: usize },

    #[error(transparent)]
    Shell(#[from] ShellError),

    #[error("search failed: {0}")]
    Search(String),
}

#[derive(Clone)]
pub struct Files {
    shell: Shell,
    max_output_chars: usize,
}

impl Files {
    pub fn new(shell: Shell, max_output_chars: usize) -> Self {
        Self {
            shell,
            max_output_chars,
        }
    }

    /// Numbered lines of the file, optionally limited to a 1-based inclusive range.
    pub fn read(
        &self,
        path: &str,
        from: Option<usize>,
        to: Option<usize>,
    ) -> Result<String, FsError> {
        let text =
            fs::read_to_string(self.resolve(path)?).map_err(|source| io_error(path, source))?;
        let total = text.lines().count();
        let from = from.unwrap_or(1).max(1);
        let to = to.unwrap_or(total).min(total);
        let body: Vec<String> = text
            .lines()
            .enumerate()
            .skip(from - 1)
            .take((to + 1).saturating_sub(from))
            .map(|(index, line)| format!("{:>5}  {line}", index + 1))
            .collect();
        let report = format!("{path} lines {from}-{to} of {total}\n{}", body.join("\n"));
        Ok(head(&report, self.max_output_chars))
    }

    /// Creates or overwrites the file, creating parent directories.
    pub fn write(&self, path: &str, content: &str) -> Result<String, FsError> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|source| io_error(path, source))?;
        }
        fs::write(&full, content).map_err(|source| io_error(path, source))?;
        Ok(format!("wrote {} bytes to {path}", content.len()))
    }

    /// Replaces one exact occurrence of `old` with `new`.
    pub fn replace(&self, path: &str, old: &str, new: &str) -> Result<String, FsError> {
        let full = self.resolve(path)?;
        let text = fs::read_to_string(&full).map_err(|source| io_error(path, source))?;
        match text.matches(old).count() {
            0 => return Err(FsError::NotFound(path.to_string())),
            1 => {}
            count => {
                return Err(FsError::Ambiguous {
                    path: path.to_string(),
                    count,
                })
            }
        }
        fs::write(&full, text.replacen(old, new, 1)).map_err(|source| io_error(path, source))?;
        Ok(format!("replaced in {path}"))
    }

    /// Entries of one directory, directories marked with a trailing slash.
    pub fn list(&self, path: &str) -> Result<String, FsError> {
        let entries = fs::read_dir(self.resolve(path)?).map_err(|source| io_error(path, source))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| io_error(path, source))?;
            let mut name = entry.file_name().to_string_lossy().to_string();
            if entry.path().is_dir() {
                name.push('/');
            }
            names.push(name);
        }
        names.sort_unstable();
        Ok(head(&names.join("\n"), self.max_output_chars))
    }

    /// Extended-regex search over tracked and untracked files, honouring .gitignore.
    pub async fn search(&self, pattern: &str, path: &str) -> Result<String, FsError> {
        let path = if path.is_empty() { "." } else { path };
        self.resolve(path)?;
        let args = ["grep", "-nIE", "--untracked", "-e", pattern, "--", path];
        let output = self.shell.output("git", &args).await?;
        match output.status.code() {
            Some(0) => Ok(head(
                String::from_utf8_lossy(&output.stdout).trim_end(),
                self.max_output_chars,
            )),
            Some(1) => Ok("no matches".to_string()),
            _ => Err(FsError::Search(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            )),
        }
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, FsError> {
        let relative = Path::new(path);
        let escapes = relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, Component::ParentDir));
        if escapes {
            return Err(FsError::Escape(path.to_string()));
        }
        Ok(self.shell.workdir().join(relative))
    }
}

fn io_error(path: &str, source: std::io::Error) -> FsError {
    FsError::Io {
        path: path.to_string(),
        source,
    }
}
