use std::path::{Component, Path, PathBuf};

use nix::unistd::Pid;

const AT_FDCWD: i32 = -100;

pub fn normalize(base: &Path, raw: &str) -> String {
    let joined = if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        base.join(raw)
    };
    let mut out = PathBuf::from("/");
    for part in joined.components() {
        match part {
            Component::Normal(seg) => out.push(seg),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out.to_string_lossy().into_owned()
}

pub fn absolute(pid: Pid, dirfd: i32, raw: &str) -> String {
    if raw.starts_with('/') {
        return normalize(Path::new("/"), raw);
    }
    let link = if dirfd == AT_FDCWD {
        format!("/proc/{pid}/cwd")
    } else {
        format!("/proc/{pid}/fd/{dirfd}")
    };
    match std::fs::read_link(&link) {
        Ok(base) => normalize(&base, raw),
        Err(_) => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_path_ignores_base() {
        assert_eq!(normalize(Path::new("/work"), "/etc/hostname"), "/etc/hostname");
    }

    #[test]
    fn relative_path_joins_base() {
        assert_eq!(normalize(Path::new("/work"), "src/main.rs"), "/work/src/main.rs");
    }

    #[test]
    fn dot_segments_disappear() {
        assert_eq!(normalize(Path::new("/work"), "./a.c"), "/work/a.c");
    }

    #[test]
    fn parent_segment_pops_one_level() {
        assert_eq!(normalize(Path::new("/work/src"), "../Cargo.toml"), "/work/Cargo.toml");
    }

    #[test]
    fn parent_above_root_stays_at_root() {
        assert_eq!(normalize(Path::new("/"), "../x"), "/x");
    }
}
