//! drawio-sync — 上游同步（规范：docs/06-upstream-sync.md）。
//!
//! 三级同步：
//! - **L1** vendor 整目录换版（drawio webapp，带回归门禁）；
//! - **L2** 文本资产 hash 比对覆盖（提示词/图标库/模型目录）；
//! - **L3** 逻辑变更 → 再移植报告（永不自动合并）。

pub mod apply;
pub mod fetch;
pub mod manifest;
pub mod report;

pub use manifest::{Manifest, Upstream, UPSTREAM_TOML};
pub use report::{Report, SyncReport, UpstreamReport};
