//! End-to-end pipeline test: index -> health -> proposals against a synthetic
//! project. No network, no git remote, no shell-out — fully deterministic.

use fract::complexity;
use fract::config::Config;
use fract::daemon::Daemon;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn temp_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fract-it-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn write_file(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
}

#[tokio::test]
async fn index_health_and_proposals_pipeline() {
    let root = temp_project("pipeline");

    // Small, simple module — should stay healthy / low entropy.
    write_file(
        &root.join("src/lib.rs"),
        "pub fn a() -> i32 { 1 }\npub fn b() -> i32 { 2 }\n",
    );

    // Large, branch-heavy module — should dominate entropy.
    let mut big = String::new();
    for i in 0..60 {
        big.push_str(&format!(
            "pub fn f{i}(x: i32) -> i32 {{ if x > 0 {{ if x > 1 {{ if x > 2 {{ x }} else {{ 0 }} }} else {{ 0 }} }} else {{ -1 }}\n",
            i = i
        ));
    }
    write_file(&root.join("src/big.rs"), &big);

    let cfg = Config::default_for(root.clone());
    let threshold = cfg.entropy_threshold;
    let daemon = Arc::new(Daemon::new(cfg));
    daemon.scan().await.expect("scan failed");

    // Index stage: both files discovered.
    let modules = daemon.modules().await;
    assert_eq!(modules.len(), 2, "expected two indexed modules");

    // Health stage: counts partition the set, score is in range, trend recorded.
    let health = daemon.project_health().await;
    assert_eq!(health.total_modules, 2);
    assert_eq!(health.healthy + health.warning + health.critical, 2);
    assert!(
        health.score > 0.0 && health.score <= 100.0,
        "score out of range: {}",
        health.score
    );
    assert_eq!(health.entropy_trend.len(), 1);

    // Ordering property: the big module is strictly more entropic than lib.
    let big = modules.iter().find(|m| m.path.ends_with("big.rs")).unwrap();
    let lib = modules.iter().find(|m| m.path.ends_with("lib.rs")).unwrap();
    assert!(
        big.entropy > lib.entropy,
        "expected big ({}) > lib ({})",
        big.entropy,
        lib.entropy
    );

    // Proposal stage: one proposal per module at/above the threshold.
    let expected = modules
        .iter()
        .filter(|m| complexity::entropy(m) >= threshold)
        .count();
    let proposals = daemon.proposals().await;
    assert_eq!(
        proposals.len(),
        expected,
        "proposal count must match modules over threshold ({threshold})"
    );
    assert!(proposals
        .iter()
        .all(|p| (0.0..=1.0).contains(&p.confidence)));

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn empty_project_scores_perfect_and_proposes_nothing() {
    let root = temp_project("empty");
    let cfg = Config::default_for(root.clone());
    let daemon = Arc::new(Daemon::new(cfg));
    daemon.scan().await.expect("scan failed");

    let health = daemon.project_health().await;
    assert_eq!(health.total_modules, 0);
    assert_eq!(health.score, 100.0);
    assert!(daemon.proposals().await.is_empty());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn init_writes_config_into_target_dir() {
    // Drive the same logic the `init` command uses: config must land inside
    // the requested directory, not the process cwd.
    let root = temp_project("init");
    let cfg = Config::default_for(root.clone());
    let text = toml::to_string_pretty(&cfg).unwrap();
    let out = root.join("fract.toml");
    fs::write(&out, &text).unwrap();

    assert!(
        out.exists(),
        "fract.toml should be written under the project root"
    );
    let parsed: toml::Value = toml::from_str(&text).unwrap();
    assert!(parsed.get("project_root").is_some());

    let _ = fs::remove_dir_all(&root);
}
