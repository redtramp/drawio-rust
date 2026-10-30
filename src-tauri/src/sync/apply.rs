//! 各层同步应用（docs/06 §4）：L1 vendor 换版、L2 文本资产 hash 比对覆盖、
//! L3 逻辑变更报告（永不自动合并）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::sync::manifest::{Manifest, Upstream};
use crate::sync::report::SyncReport;

fn file_hash(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path:?}: {e}"))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}

/// 目录 → (相对路径 → hash)。
fn dir_map(dir: &Path) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    walk_dir(dir, dir, &mut out)?;
    Ok(out)
}

fn walk_dir(base: &Path, dir: &Path, out: &mut HashMap<String, String>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir:?}: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(base, &path, out)?;
        } else {
            let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
            out.insert(rel.to_string_lossy().to_string(), file_hash(&path)?);
        }
    }
    Ok(())
}

fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    std::fs::copy(src, dst).map_err(|e| format!("copy {src:?} -> {dst:?}: {e}"))?;
    Ok(())
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    // 清空目标（保留 .git 之外的旧文件一并删除，实现"整目录换版"）
    if dst.exists() {
        std::fs::remove_dir_all(dst).map_err(|e| format!("rm {dst:?}: {e}"))?;
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir {dst:?}: {e}"))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("read_dir {src:?}: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir_contents(&s, &d)?;
        } else {
            copy_file(&s, &d)?;
        }
    }
    Ok(())
}

fn glob_files(base: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
    let full = base.join(pattern);
    let pattern_str = full.to_string_lossy().to_string();
    let mut out = Vec::new();
    for entry in glob::glob(&pattern_str).map_err(|e| format!("glob: {e}"))? {
        match entry {
            Ok(p) => out.push(p),
            Err(e) => return Err(format!("glob entry: {e}")),
        }
    }
    out.sort();
    Ok(out)
}

/// L1：整目录换版。返回 target 是否与 source 不同。
pub fn apply_l1(root: &Path, mirror: &Path, up: &Upstream, dry_run: bool) -> Result<bool, String> {
    let source = mirror.join(up.source_path.as_deref().unwrap_or("."));
    let target = root.join(up.target_path.as_deref().unwrap_or("."));
    let src_map = dir_map(&source)?;
    let tgt_map = dir_map(&target)?;
    let changed = src_map != tgt_map;
    if changed && !dry_run {
        copy_dir_contents(&source, &target)?;
    }
    Ok(changed)
}

/// L2：文本资产 hash 比对覆盖。返回 (是否变更, 变更文件列表)。
pub fn apply_l2(
    root: &Path,
    mirror: &Path,
    up: &Upstream,
    dry_run: bool,
) -> Result<(bool, Vec<String>), String> {
    let mut changed = false;
    let mut files: Vec<String> = Vec::new();
    for asset in &up.assets {
        // 单个文件
        if let (Some(source), Some(target)) = (&asset.source, &asset.target) {
            let src = mirror.join(source);
            let tgt = root.join(target);
            let same = file_hash(&src).ok() == file_hash(&tgt).ok();
            if !same {
                changed = true;
                files.push(target.clone());
                if !dry_run {
                    copy_file(&src, &tgt)?;
                }
            }
        }
        // glob → 目录（扁平拷贝，保留文件名）
        if let (Some(pattern), Some(target_dir)) = (&asset.source_glob, &asset.target_dir) {
            let matches = glob_files(mirror, pattern)?;
            let tgt_dir = root.join(target_dir);
            for m in matches {
                let fname = m.file_name().ok_or_else(|| "no filename".to_string())?;
                let tgt = tgt_dir.join(fname);
                let same = file_hash(&m).ok() == file_hash(&tgt).ok();
                if !same {
                    changed = true;
                    files.push(tgt.to_string_lossy().to_string());
                    if !dry_run {
                        copy_file(&m, &tgt)?;
                    }
                }
            }
        }
    }
    Ok((changed, files))
}

/// L3：逻辑变更报告（永不自动合并）。MVP 以 drift 为准（由调用方设置），
/// 这里返回 false 占位，保留接口。
pub fn apply_l3(_root: &Path, _mirror: &Path, _up: &Upstream) -> Result<bool, String> {
    Ok(false)
}

/// run 命令：同步（全部或指定上游）。
pub fn run(root: PathBuf, args: &[String]) -> Result<SyncReport, String> {
    let mut dry_run = false;
    let mut json = false;
    let mut ids: Vec<String> = Vec::new();
    for a in args {
        match a.as_str() {
            "--dry-run" => dry_run = true,
            "--json" => json = true,
            "--all" => {}
            s if s.starts_with("--") => return Err(format!("unknown flag {s}")),
            s => ids.push(s.to_string()),
        }
    }
    let manifest = Manifest::load(&root.join(crate::sync::manifest::UPSTREAM_TOML))?;
    let selected: Vec<String> = if ids.is_empty() {
        manifest.ordered_ids()
    } else {
        ids
    };
    let mut report = SyncReport {
        dry_run,
        upstreams: Vec::new(),
    };
    for id in selected {
        let up = manifest
            .upstream
            .get(&id)
            .ok_or_else(|| format!("unknown upstream \"{id}\""))?;
        let mut r = crate::sync::fetch::sync_upstream(&root, &id, up, dry_run)?;
        // L3 报告：drift 即提示再移植（逻辑永不自动合并）
        if up.tier.contains("l3") {
            r.l3_changed = r.drift;
        }
        report.upstreams.push(r);
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        print_report(&report);
    }
    Ok(report)
}

fn print_report(report: &SyncReport) {
    for r in &report.upstreams {
        println!(
            "[{}] head={} drift={} l1={} l2={} l3={}",
            r.id,
            &r.upstream_head[..r.upstream_head.len().min(8)],
            r.drift,
            r.l1_changed,
            r.l2_changed,
            r.l3_changed
        );
        if let Some(e) = &r.error {
            println!("    error: {e}");
        }
        for f in &r.l2_files {
            println!("    l2: {f}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l2_copies_changed_asset() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("mirror");
        std::fs::create_dir_all(mirror.join("lib")).unwrap();
        std::fs::write(mirror.join("lib/x.md"), "new\n").unwrap();

        let target = tmp.path().join("crates").join("assets");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("x.md"), "old\n").unwrap();

        let up = Upstream {
            remote: String::new(),
            branch: String::new(),
            pinned_commit: String::new(),
            tier: "l2".to_string(),
            source_path: None,
            target_path: None,
            note: None,
            assets: vec![crate::sync::manifest::Asset {
                source: Some("lib/x.md".to_string()),
                source_glob: None,
                target: Some("crates/assets/x.md".to_string()),
                target_dir: None,
            }],
            logic: vec![],
        };
        let (changed, files) = apply_l2(tmp.path(), &mirror, &up, false).unwrap();
        assert!(changed);
        assert_eq!(files.len(), 1);
        assert_eq!(
            std::fs::read_to_string(target.join("x.md")).unwrap(),
            "new\n"
        );
    }

    #[test]
    fn l2_dry_run_does_not_write() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("mirror");
        std::fs::create_dir_all(&mirror).unwrap();
        std::fs::write(mirror.join("x.md"), "new\n").unwrap();
        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("x.md"), "old\n").unwrap();
        let up = Upstream {
            remote: String::new(),
            branch: String::new(),
            pinned_commit: String::new(),
            tier: "l2".to_string(),
            source_path: None,
            target_path: None,
            note: None,
            assets: vec![crate::sync::manifest::Asset {
                source: Some("x.md".to_string()),
                source_glob: None,
                target: Some("out/x.md".to_string()),
                target_dir: None,
            }],
            logic: vec![],
        };
        let (changed, _) = apply_l2(tmp.path(), &mirror, &up, true).unwrap();
        assert!(changed);
        assert_eq!(
            std::fs::read_to_string(target.join("x.md")).unwrap(),
            "old\n"
        );
    }

    #[test]
    fn l1_detects_directory_change() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = tmp.path().join("m");
        std::fs::create_dir_all(mirror.join("src")).unwrap();
        std::fs::write(mirror.join("src/a.txt"), "1\n").unwrap();
        let target = tmp.path().join("vendor");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.txt"), "2\n").unwrap();
        let up = Upstream {
            remote: String::new(),
            branch: String::new(),
            pinned_commit: String::new(),
            tier: "l1".to_string(),
            source_path: Some("src".to_string()),
            target_path: Some("vendor".to_string()),
            note: None,
            assets: vec![],
            logic: vec![],
        };
        let changed = apply_l1(tmp.path(), &mirror, &up, false).unwrap();
        assert!(changed);
        assert_eq!(
            std::fs::read_to_string(target.join("a.txt")).unwrap(),
            "1\n"
        );
    }
}
