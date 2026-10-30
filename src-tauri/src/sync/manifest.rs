//! 同步清单解析（upstream.toml，规范 docs/06 §3）。
//!
//! 数据模型对应 `docs/04-interfaces-and-protocols.md §5.1` 的 upstream.toml。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

pub const UPSTREAM_TOML: &str = "upstream.toml";

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub upstream: HashMap<String, Upstream>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Upstream {
    pub remote: String,
    pub branch: String,
    pub pinned_commit: String,
    /// "l1" | "l2" | "l3" | "l2+l3"
    pub tier: String,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub target_path: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub logic: Vec<Logic>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default, rename = "source_glob")]
    pub source_glob: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default, rename = "target_dir")]
    pub target_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Logic {
    #[serde(rename = "source_glob")]
    pub source_glob: String,
    #[serde(rename = "maps_to")]
    pub maps_to: String,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("read {path:?}: {e}"))?;
        toml::from_str(&content).map_err(|e| format!("parse {path:?}: {e}"))
    }

    /// 上游是否启用某层（tier 含 "l1"/"l2"/"l3"）。
    pub fn has_tier(&self, id: &str, tier: &str) -> bool {
        self.upstream
            .get(id)
            .map(|u| u.tier.contains(tier))
            .unwrap_or(false)
    }

    /// 有序上游 id（HashMap 无序，按名字排序保证报告稳定）。
    pub fn ordered_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.upstream.keys().cloned().collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest() {
        let toml = r#"
[upstream.drawio]
remote = "https://github.com/jgraph/drawio.git"
branch = "dev"
pinned_commit = "abc123"
tier = "l1"
source_path = "src/main/webapp"
target_path = "public/drawio"

[upstream.next]
remote = "https://example.com/next.git"
branch = "main"
pinned_commit = "def456"
tier = "l2+l3"

[[upstream.next.assets]]
source = "lib/system-prompts.ts"
target = "src-tauri/assets/prompt/system-prompt.md"

[[upstream.next.logic]]
source_glob = "packages/**/*.ts"
maps_to = "src-tauri/src/ai/"
"#;
        let m: Manifest = toml::from_str(toml).unwrap();
        assert_eq!(m.upstream.len(), 2);
        assert!(m.has_tier("drawio", "l1"));
        assert!(!m.has_tier("drawio", "l2"));
        assert!(m.has_tier("next", "l2"));
        assert!(m.has_tier("next", "l3"));
        assert_eq!(m.upstream["next"].assets.len(), 1);
        assert_eq!(m.upstream["next"].logic.len(), 1);
    }

    #[test]
    fn ordered_ids_are_sorted() {
        let m: Manifest = toml::from_str(
            r#"
[upstream.b]
remote = "r" 
branch = "b"
pinned_commit = "c"
tier = "l1"

[upstream.a]
remote = "r"
branch = "b"
pinned_commit = "c"
tier = "l1"
"#,
        )
        .unwrap();
        assert_eq!(m.ordered_ids(), vec!["a", "b"]);
        assert!(!m.has_tier("missing", "l1"));
    }

    #[test]
    fn load_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UPSTREAM_TOML);
        std::fs::write(
            &path,
            r#"
[upstream.drawio]
remote = "https://github.com/jgraph/drawio.git"
branch = "dev"
pinned_commit = "abc123"
tier = "l1"
"#,
        )
        .unwrap();
        let m = Manifest::load(&path).unwrap();
        assert!(m.has_tier("drawio", "l1"));
        // 不存在报错
        assert!(Manifest::load(&dir.path().join("nope.toml")).is_err());
    }
}
