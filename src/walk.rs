use std::collections::HashSet;
use std::fs::{self, ReadDir};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

pub struct Walk {
    root: PathBuf,
    ignore: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub is_file: bool,
    pub is_dir: bool,
}

impl Walk {
    pub fn new(root: PathBuf, ignore: Vec<String>) -> Self {
        Self { root, ignore }
    }

    pub fn files(self) -> impl Iterator<Item = io::Result<PathBuf>> {
        WalkIter {
            root: self.root,
            ignore: self.ignore,
            stack: Vec::new(),
            #[cfg(unix)]
            seen: HashSet::new(),
            #[cfg(not(unix))]
            seen: HashSet::new(),
            started: false,
        }
    }
}

#[cfg(unix)]
type SeenId = (u64, u64);

#[cfg(not(unix))]
type SeenId = PathBuf;

struct WalkIter {
    root: PathBuf,
    ignore: Vec<String>,
    stack: Vec<ReadDir>,
    seen: HashSet<SeenId>,
    started: bool,
}

impl Iterator for WalkIter {
    type Item = io::Result<PathBuf>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.started {
            self.started = true;
            match fs::read_dir(&self.root) {
                Ok(rd) => self.stack.push(rd),
                Err(e) => return Some(Err(e)),
            }
        }

        while let Some(rd) = self.stack.last_mut() {
            match rd.next() {
                Some(Ok(entry)) => {
                    let path = entry.path();
                    let meta = match entry.metadata() {
                        Ok(m) => m,
                        Err(e) => return Some(Err(e)),
                    };
                    let file_type = meta.file_type();

                    if file_type.is_symlink() {
                        #[cfg(not(unix))]
                        {
                            // Avoid symlink loops on non-Unix by not following symlinks.
                            continue;
                        }
                        #[cfg(unix)]
                        {
                            let target = match fs::metadata(&path) {
                                Ok(m) => m,
                                Err(_) => continue,
                            };
                            let id = (target.dev(), target.ino());
                            if !self.seen.insert(id) {
                                continue;
                            }
                            if target.is_dir() {
                                let rel = path.strip_prefix(&self.root).unwrap_or(&path);
                                if self.is_ignored(rel) {
                                    continue;
                                }
                                match fs::read_dir(&path) {
                                    Ok(rd) => self.stack.push(rd),
                                    Err(e) => return Some(Err(e)),
                                }
                            } else if target.is_file() {
                                let rel = path.strip_prefix(&self.root).unwrap_or(&path);
                                if self.is_ignored(rel) {
                                    continue;
                                }
                                return Some(Ok(path));
                            }
                            continue;
                        }
                    }

                    if file_type.is_dir() {
                        #[cfg(unix)]
                        {
                            let id = (meta.dev(), meta.ino());
                            if !self.seen.insert(id) {
                                continue;
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            if !self.seen.insert(path.clone()) {
                                continue;
                            }
                        }
                        let rel = path.strip_prefix(&self.root).unwrap_or(&path);
                        if self.is_ignored(rel) {
                            continue;
                        }
                        match fs::read_dir(&path) {
                            Ok(rd) => self.stack.push(rd),
                            Err(e) => return Some(Err(e)),
                        }
                    } else if file_type.is_file() {
                        let rel = path.strip_prefix(&self.root).unwrap_or(&path);
                        if self.is_ignored(rel) {
                            continue;
                        }
                        return Some(Ok(path));
                    }
                }
                Some(Err(e)) => return Some(Err(e)),
                None => {
                    self.stack.pop();
                }
            }
        }
        None
    }
}

impl WalkIter {
    fn is_ignored(&self, rel: &Path) -> bool {
        let rel_str = rel.to_string_lossy();
        for pat in &self.ignore {
            if glob_match(&rel_str, pat) {
                return true;
            }
        }
        false
    }
}

pub fn glob_match(path: &str, pat: &str) -> bool {
    let pat = pat.trim_start_matches('/');
    if pat == "**" {
        return true;
    }
    let mut p_chars = pat.chars().peekable();
    let mut s_chars = path.chars().peekable();
    while let Some(pc) = p_chars.next() {
        match pc {
            '*' => {
                let rest: String = p_chars.clone().collect();
                if rest.is_empty() {
                    return !s_chars.clone().any(|c| c == '/');
                }
                loop {
                    if glob_match(&s_chars.clone().collect::<String>(), &rest) {
                        return true;
                    }
                    match s_chars.next() {
                        Some('/') | None => return false,
                        _ => {}
                    }
                }
            }
            '?' => {
                if s_chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if s_chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }
    s_chars.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_tree(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_all_files() {
        let root = temp_tree("walk_find");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "").unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        fs::write(root.join("readme.md"), "").unwrap();

        let mut paths: Vec<PathBuf> = Walk::new(root.clone(), Vec::new())
            .files()
            .map(|r| r.unwrap())
            .collect();
        paths.sort();

        assert_eq!(paths.len(), 3);
        assert!(paths.iter().any(|p| p.ends_with("main.rs")));
        assert!(paths.iter().any(|p| p.ends_with("lib.rs")));
        assert!(paths.iter().any(|p| p.ends_with("readme.md")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn respects_ignore_patterns() {
        let root = temp_tree("walk_ignore");
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("target/out.rs"), "").unwrap();
        fs::write(root.join("src/main.rs"), "").unwrap();

        let paths: Vec<PathBuf> = Walk::new(root.clone(), vec!["target".into()])
            .files()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("main.rs"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn glob_star_matches_any_segment() {
        assert!(glob_match("foo.rs", "*.rs"));
        assert!(!glob_match("src/foo.rs", "*.rs"));
        assert!(glob_match("src/foo.rs", "src/*.rs"));
        assert!(!glob_match("src/nested/foo.rs", "src/*.rs"));
    }

    #[test]
    fn glob_question_matches_single_char() {
        assert!(glob_match("foo.rs", "f?o.rs"));
        assert!(!glob_match("fooo.rs", "f?o.rs"));
    }

    #[test]
    fn glob_leading_slash_is_ignored() {
        assert!(glob_match("foo.rs", "/foo.rs"));
    }
}
