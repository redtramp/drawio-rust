//! 同步报告（docs/06 §5）：漂移检测与应用的机器可读结果。

use serde::Serialize;

/// 单个上游的同步报告。
#[derive(Debug, Clone, Serialize, Default)]
pub struct UpstreamReport {
    pub id: String,
    /// 上游当前 HEAD（工作区 checkout 后）。
    pub upstream_head: String,
    /// 是否有新提交（HEAD != pinned_commit）。
    pub drift: bool,
    /// L1 vendor 是否变更（整目录换版）。
    pub l1_changed: bool,
    /// L2 文本资产是否变更。
    pub l2_changed: bool,
    /// L2 变更的文件列表。
    pub l2_files: Vec<String>,
    /// L3 逻辑源是否变更（需再移植）。
    pub l3_changed: bool,
    /// 各层错误（None 表示成功）。
    pub error: Option<String>,
}

/// 一次 run 的总体报告。
#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncReport {
    pub dry_run: bool,
    pub upstreams: Vec<UpstreamReport>,
}

/// 人类可读报告。
#[derive(Debug, Clone, Serialize, Default)]
pub struct Report {
    pub sync: SyncReport,
}
