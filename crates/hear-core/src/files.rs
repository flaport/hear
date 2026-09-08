use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File, OpenOptions},
    path::{Component, Path, PathBuf},
};

fn normalized(path: &Path) -> Result<PathBuf> {
    normalize_with_budget(path, 40)
}
fn normalize_with_budget(path: &Path, remaining_links: usize) -> Result<PathBuf> {
    if remaining_links == 0 {
        bail!("too many symbolic links in {}", path.display());
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    // Resolve existing ancestors as well as lexical aliases of not-yet-created files.
    if let Ok(p) = absolute.canonicalize() {
        return Ok(p);
    }
    let mut p = PathBuf::new();
    for c in absolute.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                p.pop();
            }
            _ => p.push(c),
        }
        if let Ok(real) = p.canonicalize() {
            p = real;
        } else if let Ok(target) = fs::read_link(&p) {
            let target = if target.is_absolute() {
                target
            } else {
                p.parent().unwrap_or(Path::new("/")).join(target)
            };
            p = normalize_with_budget(&target, remaining_links - 1)?;
        }
    }
    Ok(p)
}
pub fn same_file(left: &Path, right: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (fs::metadata(left), fs::metadata(right))
            && a.dev() == b.dev()
            && a.ino() == b.ino()
        {
            return Ok(true);
        }
    }
    Ok(normalized(left)? == normalized(right)?)
}
pub fn ensure_distinct(paths: &[Option<&Path>]) -> Result<()> {
    for (i, a) in paths.iter().enumerate() {
        for b in &paths[i + 1..] {
            if let (Some(a), Some(b)) = (a, b)
                && same_file(a, b)?
            {
                bail!(
                    "input, transcript, raw output and saved recording must refer to different files: {} and {}",
                    a.display(),
                    b.display()
                );
            }
        }
    }
    Ok(())
}
pub fn preflight(path: &Path, force: bool) -> Result<()> {
    if path.exists() && (!force || !path.is_file()) {
        bail!(
            "destination already exists or is not a file: {}; use --force to replace a file",
            path.display()
        );
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        && !parent.is_dir()
    {
        bail!("parent directory does not exist: {}", parent.display());
    }
    Ok(())
}
pub fn create(path: &Path, force: bool) -> Result<File> {
    let mut o = OpenOptions::new();
    o.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    if force {
        o.create(true).truncate(true);
    } else {
        o.create_new(true);
    }
    o.open(path)
        .with_context(|| format!("could not create {}", path.display()))
}
pub fn copy(source: &Path, destination: &Path, force: bool) -> Result<()> {
    ensure_distinct(&[Some(source), Some(destination)])?;
    let mut source = File::open(source)?;
    let mut dest = create(destination, force)?;
    std::io::copy(&mut source, &mut dest)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detect_aliases_before_creation_and_hard_links() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        assert!(same_file(&a, &d.path().join("./a")).unwrap());
        #[cfg(unix)]
        {
            let link = d.path().join("dangling");
            std::os::unix::fs::symlink(&a, &link).unwrap();
            assert!(same_file(&a, &link).unwrap());
        }
        fs::write(&a, "audio").unwrap();
        let b = d.path().join("b");
        fs::hard_link(&a, &b).unwrap();
        assert!(ensure_distinct(&[Some(&a), Some(&b)]).is_err());
        assert!(copy(&a, &b, true).is_err());
        assert_eq!(fs::read_to_string(a).unwrap(), "audio");
    }
}
