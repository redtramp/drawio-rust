# drawio-rust

在 draw.io 编辑器内核之上，融合 **AI 对话生成/编辑图形**（源自 next-ai-draw-io）与 **Visio 兼容导出**（源自 drawio-to-visio）的跨平台桌面应用。

- **后端/桌面壳**：Rust + [Tauri 2]
- **前端**：Svelte 5（Vite + TypeScript + Tailwind 4）
- **编辑器内核**：draw.io webapp（vendored，iframe 同源嵌入）

## 目录

| 路径 | 说明 |
|------|------|
| `src-tauri/src` | 单一后端 crate：AI 编排 / 文档 / Visio 转换 / 会话 / 同步（`ai/ doc/ visio/ session/ sync/`）|
| `src-tauri/src/bin/drawio-sync.rs` | 上游同步 CLI（`drawio-sync`）：三级同步（L1 vendor / L2 文本资产 / L3 逻辑报告）|
| `src` | Svelte 5 前端（聊天面板/画布/设置/同步面板） |
| `public/drawio` | vendored draw.io webapp（由 `drawio-sync` L1 维护，勿手改） |
| `src-tauri/assets/prompt` | 提示词/图标库/模型目录（`drawio-sync` L2 同步目标）|
| `sync/` | 同步运行时（`vendor-keep/patches` 本地补丁、`reports/` 同步报告） |
| `upstream.toml` | 上游同步清单（pinned commits + 层级映射） |
| `docs/` | 规范工程文档（需求/架构/模块设计/接口/实施计划/同步机制/调研） |

## 开发

```sh
# 前端（dev server, :1420）
npm install && npm run dev

# Rust（核心逻辑测试）
cargo test --workspace

# 桌面应用
npm install
cargo tauri dev        # 开发
cargo tauri build      # 打包
```

## 上游同步

```sh
cargo run --bin drawio-sync -- run --all            # 同步三上游
cargo run --bin drawio-sync -- run --all --dry-run  # 只看漂移
cargo run --bin drawio-sync -- status               # 查看状态
```

同步语义与兜底流程见 `docs/06-upstream-sync.md`。

## 许可

- draw.io webapp：Apache-2.0（见 `public/drawio/LICENSE`）；本产品与 draw.io 官方无关联（商标不授予）。
- 提示词/图标库资产：源自 Apache-2.0 的 next-ai-draw-io，来源头注释保留于 `src-tauri/assets/prompt/`。
- 本产品：Apache-2.0（见根 LICENSE）。
