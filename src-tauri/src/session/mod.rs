//! 会话版本快照（内存，S6）。
//!
//! 每次 AI 在画布上产生新图（display/edit）时记录一版快照，上限 20。每版附
//! 相对上一版的 cell 级 diff 计数（供版本卡展示「对比」）。磁盘持久化（会话
//! 恢复）见 S11。

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ai::diff::{diff_diagrams, is_same_document, ChangeSummary};
use crate::error::Error;

/// 版本快照上限（对齐 next-ai-draw-io 的版本策略，ADR-8）。
pub const MAX_VERSIONS: usize = 20;

/// 单条版本快照。`summary` 为相对上一版的变更计数（首版为 from_scratch）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSnapshot {
    /// 版本 id（uuid）。
    pub id: String,
    /// 变更来源描述（display_diagram / edit_diagram / manual）。
    pub label: String,
    /// 完整 mxfile XML（恢复时用；列表接口可省略以减小载荷）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xml: Option<String>,
    /// 相对上一版的变更计数。
    pub summary: ChangeSummary,
    /// 首个版本（画布从无到有）。
    pub from_scratch: bool,
    /// 记录序号（1 起，稳定递增，用于展示顺序）。
    pub seq: usize,
}

/// 内存版本快照存储。
#[derive(Debug, Default)]
pub struct VersionStore {
    snapshots: Vec<VersionSnapshot>,
}

impl VersionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一版快照；若与最新版相同则跳过（返回 None）。超出上限淘汰最旧。
    pub fn push(&mut self, label: impl Into<String>, xml: &str) -> Option<VersionSnapshot> {
        if let Some(last) = self.snapshots.last() {
            if is_same_document(last.xml.as_deref().unwrap_or(""), xml) {
                return None;
            }
        }
        let previous = self
            .snapshots
            .last()
            .map(|s| s.xml.as_deref().unwrap_or(""));
        let (summary, from_scratch) = match previous {
            Some(prev) => {
                let d = diff_diagrams(prev, xml, None);
                (d.summary, d.from_scratch)
            }
            None => (ChangeSummary::default(), true),
        };
        let seq = self.snapshots.last().map(|s| s.seq).unwrap_or(0) + 1;
        let snapshot = VersionSnapshot {
            id: uuid::Uuid::new_v4().to_string(),
            label: label.into(),
            xml: Some(xml.to_string()),
            summary,
            from_scratch,
            seq,
        };
        self.snapshots.push(snapshot.clone());
        if self.snapshots.len() > MAX_VERSIONS {
            self.snapshots.remove(0);
        }
        Some(snapshot)
    }

    /// 版本列表（不含 xml，按记录顺序，最早在前）。
    pub fn list(&self) -> Vec<VersionSnapshot> {
        self.snapshots
            .iter()
            .map(|s| VersionSnapshot {
                xml: None,
                ..s.clone()
            })
            .collect()
    }

    /// 按 id 取完整快照。
    pub fn get(&self, id: &str) -> Option<&VersionSnapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }
}

/// 会话消息记录（用于恢复聊天面板展示，S11）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatRecord {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

/// 会话消息日志路径：`<data_dir>/sessions/<id>/messages.jsonl`。
pub fn messages_path(data_dir: &Path, session_id: &str) -> PathBuf {
    data_dir
        .join("sessions")
        .join(session_id)
        .join("messages.jsonl")
}

/// 追加一行消息记录（每行一条 JSON）。
pub fn append_message(data_dir: &Path, session_id: &str, record: &ChatRecord) -> Result<(), Error> {
    let path = messages_path(data_dir, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::Io)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(Error::Io)?;
    let line = serde_json::to_string(record).map_err(Error::Json)?;
    writeln!(file, "{line}").map_err(Error::Io)?;
    Ok(())
}

/// 读取会话消息记录（损坏行跳过，保证恢复鲁棒）。
pub fn load_messages(data_dir: &Path, session_id: &str) -> Vec<ChatRecord> {
    let path = messages_path(data_dir, session_id);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<ChatRecord>(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(n: &str) -> String {
        format!(
            r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="{n}"/></root></mxGraphModel></diagram></mxfile>"#
        )
    }

    #[test]
    fn push_creates_numbered_snapshots() {
        let mut store = VersionStore::new();
        assert!(store.push("display_diagram", &doc("A")).is_some());
        assert!(store.push("display_diagram", &doc("B")).is_some());
        assert_eq!(store.len(), 2);
        let list = store.list();
        assert_eq!(list[0].seq, 1);
        assert_eq!(list[1].seq, 2);
        assert!(list[0].from_scratch);
        assert!(!list[1].from_scratch);
    }

    #[test]
    fn push_skips_identical() {
        let mut store = VersionStore::new();
        store.push("display_diagram", &doc("A"));
        assert!(store.push("display_diagram", &doc("A")).is_none());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn push_caps_at_max_versions() {
        let mut store = VersionStore::new();
        for i in 0..(MAX_VERSIONS + 5) {
            store.push("edit_diagram", &doc(&i.to_string()));
        }
        assert_eq!(store.len(), MAX_VERSIONS);
        // 淘汰最旧，seq 从 6 起
        assert_eq!(store.list().first().unwrap().seq, 6);
    }

    #[test]
    fn get_returns_full_xml() {
        let mut store = VersionStore::new();
        let s = store.push("display_diagram", &doc("A")).unwrap();
        let fetched = store.get(&s.id).unwrap();
        assert!(fetched.xml.as_deref().unwrap().contains(r#"value="A""#));
        // 列表接口不含 xml
        assert!(store.list()[0].xml.is_none());
    }
}
