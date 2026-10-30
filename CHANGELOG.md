# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [Unreleased]

### Added
- **目录重组**：前端从 `app/` 提升到项目根（Tauri 官方约定）；`crates/drawio-core` + `crates/drawio-sync` 合并为单一 `src-tauri` 后端 crate（推翻 ADR-2，用户明确指令）。
- **文件**（S3）：打开/保存 `.drawio`（明文 XML 与 zip+raw-deflate 压缩双格式），画布打开/保存按钮。
- **版本快照**（S6）：`VersionStore` 内存快照（上限 20，相对上一版 cell 级 diff 计数）；前端版本卡列表 + 恢复；`DiagramReady` 自动记录并发 `version_new`。
- **国际化**（S11，FR-S3）：轻量 i18n store（中/英），覆盖 TopBar/ChatPanel/DrawioFrame/VersionCard/SettingsDialog/SyncPanel，顶栏语言切换。
- **首次引导**（S11）：`Settings.onboarded` 标记，首次启动自动打开 Provider 配置向导。
- **会话恢复**（S11）：`messages.jsonl` 持久化 user/assistant 消息 + `session_history` 命令 + 前端启动恢复。
- **同源桥接**（FR-E4/E6/A8）：`SameOriginBridge` 直控 draw.io `EditorUi`——AI 改动经 `ReplaceDiagram` 单步 undo、画布选中 shape 读取（`canvas.selection` → `[Selected cells: …]`）、`touched_ids` 高亮（`mxCellHighlight`）；跨域/未就绪静默降级到 postMessage。
- **thinking 展示**（FR-A10）：ChatPanel 渲染思考过程为可折叠块（每消息独立展开）。
- **文件上传生成**（FR-A7）：上传图片（截图/图片）作为多模态输入生成图形；前端上传按钮 + 缩略图预览，后端 `TurnInput.images` → image block。
- **导出选项接线**（FR-X3）：`DrawioFrame` 读取 `settings.export.scale/border` 替代硬编码。
- **主进程日志**（NFR-7）：`run()` 初始化 `tracing_subscriber`（`RUST_LOG` 可调）。

### Fixed
- 恢复被误删的 `docs/` 规范文档（14 分册）。
- 同源 `commitDiagram` 后同步 `DrawioBridge` 缓存，避免下次 AI 上下文读到旧 XML。
- `cargo run` 二进制歧义：加 `default-run = "drawio-rust"`，修复 `tauri dev` 启动失败。
- 修正 `docs/03-5` 后端设计中残留的旧三 crate 结构描述。

## [0.1.0] - 2026-10-29

### Added
- 项目脚手架：Cargo workspace（drawio-core / drawio-sync / src-tauri）+ Svelte 5 前端 + CI。
- vendored draw.io webapp（pin `9034053`，`public/drawio/`）。
- 上游同步清单 `upstream.toml`（三上游 pinned commits）。
- 工程文档 `docs/`（13 分册：需求/架构/模块设计/接口/实施计划/同步机制/调研）。
