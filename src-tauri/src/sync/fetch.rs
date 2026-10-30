//! 上游 git 抓取与漂移检测（docs/06 §4）。
//!
//! 镜像放 `.sync/mirrors/<id>`，checkout 到 pinned_commit 以便提取源文件；
//! 漂移 = `origin/<branch>` HEAD ≠ pinned_commit。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::sync::manifest::{Manifest, Upstream};
use crate::sync::report::{SyncReport, UpstreamReport};

/// 镜像目录。
pub fn mirror_dir(root: &Path, id: &str) -> PathBuf {
    root.join(".sync").join("mirrors").join(id)
}

fn git(args: &[&str], cwd: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git spawn: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 确保镜像存在并 fetch 到最新；返回镜像目录（工作树在 pinned_commit）。
pub fn ensure_mirror(root: &Path, id: &str, up: &Upstream) -> Result<PathBuf, String> {
    let dir = mirror_dir(root, id);
    if dir.join(".git").exists() {
        git(&["fetch", "origin"], &dir)?;
    } else {
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        git(
            &[
                "clone",
                "--no-checkout",
                &up.remote,
                dir.to_str().unwrap_or_default(),
            ],
            root,
        )?;
    }
    // checkout 到 pinned_commit（detached，得到该版本的工作树）
    git(&["checkout", "--detach", &up.pinned_commit], &dir)?;
    Ok(dir)
}

/// 同步单个上游（fetch + 漂移检测 + 各层应用）。dry_run 只检测不写。
pub fn sync_upstream(
    root: &Path,
    id: &str,
    up: &Upstream,
    dry_run: bool,
) -> Result<UpstreamReport, String> {
    let mut report = UpstreamReport {
        id: id.to_string(),
        ..Default::default()
    };
    let dir = match ensure_mirror(root, id, up) {
        Ok(d) => d,
        Err(e) => {
            report.error = Some(e);
            return Ok(report);
        }
    };
    let upstream_head = git(&["rev-parse", &format!("origin/{}", up.branch)], &dir)?;
    report.upstream_head = upstream_head.clone();
    report.drift = upstream_head != up.pinned_commit;

    if up.tier.contains("l1") {
        report.l1_changed = crate::sync::apply::apply_l1(root, &dir, up, dry_run)?;
    }
    if up.tier.contains("l2") {
        let (changed, files) = crate::sync::apply::apply_l2(root, &dir, up, dry_run)?;
        report.l2_changed = changed;
        report.l2_files = files;
    }
    if up.tier.contains("l3") {
        report.l3_changed = crate::sync::apply::apply_l3(root, &dir, up)?;
    }
    Ok(report)
}

/// status：只检测漂移，不应用。
pub fn status(root: PathBuf, ids: &[String]) -> Result<SyncReport, String> {
    let manifest = Manifest::load(&root.join(crate::sync::manifest::UPSTREAM_TOML))?;
    let selected: Vec<String> = if ids.is_empty() {
        manifest.ordered_ids()
    } else {
        ids.to_vec()
    };
    let mut report = SyncReport {
        dry_run: true,
        upstreams: Vec::new(),
    };
    for id in selected {
        let up = manifest
            .upstream
            .get(&id)
            .ok_or_else(|| format!("unknown upstream \"{id}\""))?;
        // 只 fetch + 漂移检测，不应用各层（L1/L2/L3 的变更在 run 里才评估）
        let dir = ensure_mirror(&root, &id, up)?;
        let head = git(&["rev-parse", &format!("origin/{}", up.branch)], &dir)?;
        report.upstreams.push(UpstreamReport {
            id: id.clone(),
            upstream_head: head.clone(),
            drift: head != up.pinned_commit,
            ..Default::default()
        });
    }
    Ok(report)
}

/// doctor：检查 git 可用性。
pub fn doctor() -> Result<(), String> {
    match Command::new("git").arg("--version").output() {
        Ok(out) if out.status.success() => {
            eprintln!("git: {}", String::from_utf8_lossy(&out.stdout).trim());
            Ok(())
        }
        _ => Err("git not found in PATH".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_upstream(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        git(&["init", "-q"], dir).unwrap();
        git(&["config", "user.email", "test@example.com"], dir).unwrap();
        git(&["config", "user.name", "test"], dir).unwrap();
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        git(&["add", "-A"], dir).unwrap();
        git(&["commit", "-q", "-m", "init"], dir).unwrap();
        let _ = git(&["checkout", "-q", "-b", "main"], dir);
    }

    #[test]
    fn detects_drift() {
        let tmp = tempfile::tempdir().unwrap();
        let up_dir = tmp.path().join("upstream");
        init_upstream(&up_dir);
        let head = git(&["rev-parse", "HEAD"], &up_dir).unwrap();

        let up = Upstream {
            remote: up_dir.to_string_lossy().to_string(),
            branch: "main".to_string(),
            pinned_commit: head.clone(),
            tier: "l1".to_string(),
            source_path: Some(".".to_string()),
            target_path: Some("vendor".to_string()),
            note: None,
            assets: vec![],
            logic: vec![],
        };
        // 无漂移
        let r1 = sync_upstream(tmp.path(), "t", &up, true).unwrap();
        assert!(!r1.drift);

        // 上游推进 → 漂移
        std::fs::write(up_dir.join("b.txt"), "world\n").unwrap();
        git(&["add", "-A"], &up_dir).unwrap();
        git(&["commit", "-q", "-m", "second"], &up_dir).unwrap();
        let r2 = sync_upstream(tmp.path(), "t", &up, true).unwrap();
        assert!(r2.drift);
    }

    #[test]
    fn mirror_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let up_dir = tmp.path().join("upstream");
        init_upstream(&up_dir);
        let head = git(&["rev-parse", "HEAD"], &up_dir).unwrap();
        let up = Upstream {
            remote: up_dir.to_string_lossy().to_string(),
            branch: "main".to_string(),
            pinned_commit: head,
            tier: "l1".to_string(),
            source_path: None,
            target_path: None,
            note: None,
            assets: vec![],
            logic: vec![],
        };
        ensure_mirror(tmp.path(), "t", &up).unwrap();
        assert!(mirror_dir(tmp.path(), "t").join(".git").exists());
    }
}
