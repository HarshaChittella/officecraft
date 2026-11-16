//! The project folder as a jail: every path the AI names is resolved inside it, and a path that
//! leads out (`..`, an absolute path elsewhere, a symbolic link pointing outside) is refused, as
//! PdfCraft's `--root` does.

use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Jail {
    root: PathBuf,
}

impl Jail {
    pub fn new(root: &Path) -> std::io::Result<Jail> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "the project must be a folder"));
        }
        Ok(Jail { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `name` (relative to the project, or absolute inside it) as a real path inside the
    /// project. A file that doesn't exist yet is fine if its folder is inside.
    pub fn resolve(&self, name: &str) -> Result<PathBuf, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a file name is needed".into());
        }
        let p = Path::new(name);
        let joined = if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) };
        // Resolve what exists (following links), then add the parts that don't exist yet; a
        // `..` among those could still step out, so refuse it.
        let mut existing = joined.clone();
        let mut rest: Vec<std::ffi::OsString> = Vec::new();
        while !existing.exists() {
            match (existing.file_name(), existing.parent()) {
                (Some(n), Some(parent)) => {
                    rest.push(n.to_os_string());
                    existing = parent.to_path_buf();
                }
                _ => return Err(format!("{name} isn't in the project")),
            }
        }
        let mut real = existing.canonicalize().map_err(|e| format!("{name}: {e}"))?;
        for part in rest.iter().rev() {
            if Path::new(part).components().any(|c| !matches!(c, Component::Normal(_))) {
                return Err(format!("{name} isn't in the project"));
            }
            real.push(part);
        }
        if !real.starts_with(&self.root) {
            return Err(format!("{name} is outside the project folder ({})", self.root.display()));
        }
        Ok(real)
    }

    /// `path` relative to the project, `/`-separated.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[cfg(unix)]
    #[test]
    fn paths_stay_in_the_project() {
        let base = std::env::temp_dir().join(format!("cw-jail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("project");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/a.docx"), b"x").unwrap();
        std::fs::write(base.join("secret.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(base.join("secret.txt"), root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(root.join("docs/a.docx"), root.join("inside.docx")).unwrap();
        let j = Jail::new(&root).unwrap();
        let real = |p: &str| j.resolve(p).unwrap();
        assert_eq!(real("docs/a.docx"), j.root().join("docs/a.docx"));
        assert_eq!(real("new/report.docx"), j.root().join("new/report.docx"), "a file to make is fine");
        assert_eq!(real(&j.root().join("docs/a.docx").to_string_lossy()), j.root().join("docs/a.docx"), "absolute inside is fine");
        assert_eq!(real("inside.docx"), j.root().join("docs/a.docx"), "a link that stays inside is fine");
        for bad in ["../secret.txt", "docs/../../secret.txt", "/etc/passwd", "link.txt", "missing/../../x", ""] {
            assert!(j.resolve(bad).is_err(), "{bad:?} must be refused");
        }
        assert_eq!(j.relative(&j.root().join("docs/a.docx")), "docs/a.docx");
    }
}
