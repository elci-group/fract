//! `walkdir`-free recursive directory walker with gitignore-style ignore
//! filtering. On Unix, symlink loops are prevented by tracking each
//! entered directory's `(dev, ino)`; elsewhere symlinks are not followed
//! at all.

use std::collections::HashSet;
use std::fs::{self, ReadDir};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

/// Dependency-free recursive directory walker.
///
/// On Unix, symlink loops are prevented by tracking `(dev, ino)` of every
/// directory entered; a directory whose inode was already seen is skipped.
/// On non-Unix platforms symlinks are **not followed at all**, which avoids
/// loops conservatively at the cost of not traversing symlinked directories.
pub struct Walk {
    root: PathBuf,
    ignore: Vec<String>,
    max_depth: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub is_file: bool,
    pub is_dir: bool,
}

impl Walk {
    /// Create a walker rooted at `root` with ignore glob patterns.
    #[must_use]
    pub fn new(root: PathBuf, ignore: Vec<String>) -> Self {
        Self {
            root,
            ignore,
            max_depth: None,
        }
    }

    /// Limit recursion to `depth` levels below the root (root's direct children
    /// are depth 1). `None` (the default) means unlimited.
    #[must_use]
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    pub fn files(self) -> impl Iterator<Item = io::Result<PathBuf>> {
        WalkIter {
            root: self.root,
            ignore: self.ignore,
            max_depth: self.max_depth,
            stack: Vec::new(),
            depths: Vec::new(),
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
    max_depth: Option<usize>,
    stack: Vec<ReadDir>,
    depths: Vec<usize>,
    seen: HashSet<SeenId>,
    started: bool,
}

impl WalkIter {
    /// Whether we may descend from a frame at `cur_depth` into its child dir.
    fn may_descend(&self, cur_depth: usize) -> bool {
        match self.max_depth {
            Some(m) => cur_depth < m,
            None => true,
        }
    }

    /// Ignore-check a candidate directory and push it onto the stack when we
    /// may descend into it. Unreadable directories (permission denied) are
    /// skipped; both call sites are at the end of the loop body, so skipping
    /// here is equivalent to the original `continue`.
    fn descend_into(&mut self, path: &Path, cur_depth: usize) {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        if self.is_ignored(rel) {
            return;
        }
        if self.may_descend(cur_depth) {
            // Err (permission denied): skip the directory.
            if let Ok(rd) = fs::read_dir(path) {
                self.stack.push(rd);
                self.depths.push(cur_depth + 1);
            }
        }
    }
}

impl Iterator for WalkIter {
    type Item = io::Result<PathBuf>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.started {
            self.started = true;
            match fs::read_dir(&self.root) {
                Ok(rd) => {
                    self.stack.push(rd);
                    self.depths.push(1);
                }
                Err(e) => return Some(Err(e)),
            }
        }

        while let Some(rd) = self.stack.last_mut() {
            let cur_depth = *self.depths.last().unwrap_or(&1);
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
                            let Ok(target) = fs::metadata(&path) else {
                                continue;
                            };
                            let id = (target.dev(), target.ino());
                            if !self.seen.insert(id) {
                                continue;
                            }
                            if target.is_dir() {
                                self.descend_into(&path, cur_depth);
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
                        self.descend_into(&path, cur_depth);
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
                    self.depths.pop();
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

/// Match a relative path against a glob pattern (`**`, `*`, `?`).
#[must_use]
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

    #[test]
    fn max_depth_limits_recursion() {
        let root = temp_tree("walk_depth");
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::write(root.join("top.txt"), "").unwrap();
        fs::write(root.join("a/one.txt"), "").unwrap();
        fs::write(root.join("a/b/two.txt"), "").unwrap();
        fs::write(root.join("a/b/c/three.txt"), "").unwrap();

        let collect = |depth: usize| -> Vec<String> {
            let mut v: Vec<String> = Walk::new(root.clone(), Vec::new())
                .with_max_depth(depth)
                .files()
                .map(|r| {
                    r.unwrap()
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            v.sort();
            v
        };

        assert_eq!(collect(1), vec!["top.txt"]);
        assert_eq!(collect(2), vec!["a/one.txt", "top.txt"]);
        assert_eq!(collect(3), vec!["a/b/two.txt", "a/one.txt", "top.txt"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_subdir_does_not_abort_walk() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_tree("walk_perm");
        fs::create_dir_all(root.join("open")).unwrap();
        fs::create_dir_all(root.join("locked")).unwrap();
        fs::write(root.join("open/yes.txt"), "").unwrap();
        fs::write(root.join("locked/secret.txt"), "").unwrap();
        fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();

        // As a non-root user the locked dir is skipped; as root it is read.
        // Either way the walk must complete and surface the readable file.
        let results: Vec<_> = Walk::new(root.clone(), Vec::new()).files().collect();
        let paths: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
        assert!(paths.iter().any(|p| p.ends_with("open/yes.txt")));

        // Restore so cleanup can remove the dir.
        let _ = fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_files_and_dirs_are_walked_without_looping() {
        use std::os::unix::fs::symlink;
        let root = temp_tree("walk_symlink");
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("real/inner.txt"), "").unwrap();
        fs::write(root.join("top.txt"), "").unwrap();
        symlink(root.join("real/inner.txt"), root.join("link_file.txt")).unwrap();
        symlink(root.join("real"), root.join("link_dir")).unwrap();
        // A loop (a -> b -> a) must not hang the walker; the dangling target
        // fails metadata and is skipped.
        symlink(root.join("a_loop"), root.join("b_loop")).unwrap();
        symlink(root.join("b_loop"), root.join("a_loop")).unwrap();

        let mut paths: Vec<PathBuf> = Walk::new(root.clone(), Vec::new())
            .files()
            .map(|r| r.unwrap())
            .collect();
        paths.sort();
        assert!(paths.iter().any(|p| p.ends_with("top.txt")));
        assert!(paths.iter().any(|p| p.ends_with("link_file.txt")));
        // `real` and `link_dir` share an inode: whichever the walker reaches
        // first wins, so `inner.txt` appears under at least one of them.
        let inner_count = paths.iter().filter(|p| p.ends_with("inner.txt")).count();
        assert!((1..=2).contains(&inner_count), "paths: {paths:?}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_root_yields_a_single_error_entry() {
        let root = temp_tree("walk_missing");
        fs::remove_dir_all(&root).unwrap();
        let mut iter = Walk::new(root, Vec::new()).files();
        assert!(iter.next().unwrap().is_err());
        assert!(iter.next().is_none());
    }

    // --- glob_match property/invariant suites (hand-rolled, deterministic LCG) ---

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// Single path segment: 1-8 lowercase letters/digits (no glob chars, no `/`).
    fn gen_segment(rng: &mut Rng) -> String {
        const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let len = 1 + usize::try_from(rng.below(8)).unwrap();
        (0..len)
            .map(|_| char::from(ALPHA[usize::try_from(rng.below(ALPHA.len() as u64)).unwrap()]))
            .collect()
    }

    /// Relative path of 1-4 segments.
    fn gen_path(rng: &mut Rng) -> String {
        let segments = 1 + usize::try_from(rng.below(4)).unwrap();
        (0..segments)
            .map(|_| gen_segment(rng))
            .collect::<Vec<_>>()
            .join("/")
    }

    #[test]
    fn prop_glob_literal_paths_match_themselves() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..50 {
            let p = gen_path(&mut rng);
            assert!(glob_match(&p, &p), "literal path {p} must match itself");
            // A pair with a differing literal or different segment count must
            // not match: without wildcards glob_match is exact string equality.
            let mut segments: Vec<String> = p.split('/').map(str::to_string).collect();
            if rng.below(2) == 0 {
                let i = usize::try_from(rng.below(segments.len() as u64)).unwrap();
                segments[i].push('z');
            } else {
                segments.push(gen_segment(&mut rng));
            }
            let q = segments.join("/");
            assert_ne!(p, q);
            assert!(
                !glob_match(&p, &q),
                "distinct literal paths {p} and {q} must not match"
            );
        }
    }

    #[test]
    fn prop_glob_star_ext_matches_single_segment_only() {
        let mut rng = Rng(0x1234_5678_9abc_def0);
        for _ in 0..50 {
            let path = format!("{}.rs", gen_segment(&mut rng));
            assert!(glob_match(&path, "*.rs"), "{path} must match *.rs");
            // The same name nested one level down must never match.
            let nested = format!("{}/{path}", gen_segment(&mut rng));
            assert!(!glob_match(&nested, "*.rs"), "{nested} must not match *.rs");
        }
        assert!(!glob_match("foo.md", "*.rs"));
        assert!(!glob_match("foo", "*.rs"));
    }

    #[test]
    fn prop_glob_double_star_matches_everything() {
        assert!(glob_match("", "**"));
        assert!(glob_match("a", "**"));
        assert!(glob_match("a/b/c/d", "**"));
        let mut rng = Rng(0xabcd_ef01_2345_6789);
        for _ in 0..50 {
            let p = gen_path(&mut rng);
            assert!(glob_match(&p, "**"), "{p} must match **");
        }
    }

    #[test]
    fn prop_glob_leading_slash_is_ignored() {
        let mut rng = Rng(0x0fed_cba9_8765_4321);
        let patterns = ["*.rs", "src/*.rs", "a?c", "**", "literal/path"];
        for _ in 0..50 {
            let p = gen_path(&mut rng);
            for pat in patterns {
                let slashed = format!("/{pat}");
                assert_eq!(
                    glob_match(&p, pat),
                    glob_match(&p, &slashed),
                    "leading slash changed result for path {p} pattern {pat}"
                );
            }
            // Also a generated literal pattern (literal-vs-literal case).
            let literal = gen_path(&mut rng);
            let slashed = format!("/{literal}");
            assert_eq!(
                glob_match(&p, &literal),
                glob_match(&p, &slashed),
                "leading slash changed result for path {p} pattern {literal}"
            );
        }
    }

    #[test]
    fn prop_glob_question_consumes_exactly_one_char() {
        let mut rng = Rng(0x5555_aaaa_3333_cccc);
        for _ in 0..50 {
            // Segments are ASCII, so byte indices are char boundaries.
            let seg = gen_segment(&mut rng);
            let i = usize::try_from(rng.below(seg.len() as u64)).unwrap();
            let pat = format!("{}?{}", &seg[..i], &seg[i + 1..]);
            assert!(
                glob_match(&seg, &pat),
                "? must match the single char at {i} in {seg} (pattern {pat})"
            );
            // Every pattern char consumes exactly one string char, so any
            // candidate whose length differs from the pattern can never match.
            let shorter = format!("{}{}", &seg[..i], &seg[i + 1..]);
            let longer = format!("{}x{}", &seg[..=i], &seg[i + 1..]);
            assert!(
                !glob_match(&shorter, &pat),
                "shorter candidate {shorter} must not match {pat}"
            );
            assert!(
                !glob_match(&longer, &pat),
                "longer candidate {longer} must not match {pat}"
            );
        }
    }

    #[test]
    fn prop_glob_adversarial_patterns_never_panic() {
        let long_literal = "a".repeat(200);
        let many_stars = "*".repeat(50);
        let long_path = "a".repeat(16);
        let patterns = [
            "******",
            "*?*?*",
            "a********b",
            "??????????",
            "**a**b**",
            "*a*a*a*a*a*",
            long_literal.as_str(),
            many_stars.as_str(),
        ];
        let paths = [
            "",
            "a",
            "ab",
            "aaaaab",
            "aaaaaa",
            "abababab",
            "a/a/a",
            "nested/deep/path",
            "xxxxxxxxxxxx",
            long_path.as_str(),
        ];
        for pat in patterns {
            for path in paths {
                let result = std::panic::catch_unwind(|| glob_match(path, pat));
                assert!(
                    result.is_ok(),
                    "glob_match panicked on path {path:?} pattern {pat:?}"
                );
            }
        }
    }
}
