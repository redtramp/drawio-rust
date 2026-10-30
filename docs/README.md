# drawio-rust 工程文档

> **drawio-rust**：在 draw.io 编辑器内核之上，融合 **AI 生成/编辑图形**（源自 next-ai-draw-io）与 **Visio 兼容导出**（源自 drawio-to-visio）的桌面应用。技术栈：**Rust + Tauri 2**（后端/桌面壳）+ **Svelte 5**（前端）。

本文档集是 drawio-rust 的规范工程文档，覆盖：需求分析、总体架构、模块设计、接口协议、分步实施计划、上游同步机制，以及对三个上游项目的调研报告。所有文档基于 2026-10-29 对 `sample/` 下三个上游仓库的实际源码调研（含 file:line 引用）。

---

## 文档地图

| 编号 | 文档 | 内容 | 读者 | 状态 |
|------|------|------|------|------|
| — | [README.md](README.md) | 文档总览与导航（本页） | 所有人 | 草案 |
| 01 | [需求分析](01-requirements.md) | 背景、目标、功能/非功能需求、约束、分期范围（MVP→v1.0） | 产品/架构 | 草案 |
| 02 | [总体架构](02-architecture.md) | 技术选型、分层与模块图、关键架构决策（ADR）、核心数据流、仓库布局 | 架构/后端/前端 | 草案 |
| 03.1 | [AI 生成模块](03-module-design/03-1-ai-engine.md) | 工具协议、系统提示词、结构校验/自动修复、VLM 视觉校验、重试状态机、Provider 抽象 | 后端 | 草案 |
| 03.2 | [编辑器集成](03-module-design/03-2-editor-integration.md) | iframe 嵌入 draw.io、postMessage 协议、同源桥接（undo/选区/高亮）、导出 | 前端/后端 | 草案 |
| 03.3 | [Visio 导出模块](03-module-design/03-3-visio-export.md) | foreignObject→text 转换算法、Rust 实现、导出管线 | 后端 | 草案 |
| 03.4 | [前端框架](03-module-design/03-4-frontend.md) | Svelte 5 应用结构、布局、聊天面板、状态管理、设置 | 前端 | 草案 |
| 03.5 | [后端服务](03-module-design/03-5-backend.md) | Rust crate 结构、AI 编排、文档服务、导出服务、同步工具、Tauri 命令 | 后端 | 草案 |
| 04 | [接口与协议](04-interfaces-and-protocols.md) | Tauri 命令/事件、postMessage 协议、LLM 工具 Schema、同步清单格式 | 前后端 | 草案 |
| 05 | [实施计划](05-implementation-plan.md) | 分步（one-PR 粒度）实施计划：依赖、并行、验证命令、退出标准、回滚 | 全体/交付 | 草案 |
| 06 | [上游同步机制](06-upstream-sync.md) | 三级同步（vendor/文本/逻辑）、同步清单、`drawio-sync` 工具设计、工作流 | 后端/运维 | 草案 |
| 07.1 | [调研：drawio](07-research/07-1-drawio.md) | draw.io 源码布局、内核、构建、导出管线、可嵌入性、许可 | 架构 | 已完成 |
| 07.2 | [调研：next-ai-draw-io](07-research/07-2-next-ai-draw-io.md) | AI 管线、工具协议、校验、编辑器桥接、可移植性分类 | 后端/前端 | 已完成 |
| 07.3 | [调研：drawio-to-visio](07-research/07-3-drawio-to-visio.md) | Visio 兼容转换问题与算法、可移植性 | 后端 | 已完成 |

**建议阅读顺序**：`01 → 02 → 03.x → 04 → 05`，调研文档（07.x）作为架构决策（02 中的 ADR）的佐证材料按需查阅。

---

## 与三个上游项目的关系

drawio-rust **不是**任一上游的拷贝，而是一个**再架构（re-architecture）**：从上游**提取能力**，按 Rust+Tauri2+Svelte5 重新组织。

| 上游项目 | 上游仓库 | 从它继承什么 | 继承方式 |
|----------|----------|--------------|----------|
| **drawio** | `github.com/jgraph/drawio` (branch `dev`) | 编辑器内核（画布、编辑、全部图形能力、导出） | **整包 vendor** 其 webapp 静态资源，iframe 嵌入 |
| **next-ai-draw-io** | `github.com/DayuanJiang/next-ai-draw-io` (branch `main`) | AI 生成/编辑能力（工具协议、提示词、结构校验、VLM 校验、Provider 抽象） | **纯逻辑译 Rust** + **提示词/领域知识原样搬** + UI **用 Svelte5 重建**（非 React 移植） |
| **drawio-to-visio** | `github.com/PanJitao/drawio-to-visio` (branch `main`) | Visio 兼容 SVG 导出（foreignObject→text） | **Python 脚本译 Rust** |

> 关键边界：next-ai-draw-io 的 React UI（chat-engine、drawio-frame 等）**不移植**，只重建为 Svelte 5；draw.io 的 3.3 万行 `EditorUi.js` **不重写**，整包嵌入。详见 [02-architecture.md §2.4 ADR](02-architecture.md)。

---

## 文档约定

- 文件名采用 **ASCII 编号**（便于 git/工具链），正文为**中文**（技术术语保留英文）。
- 每个文档头部标注**状态**（草案/评审中/已定稿）与**最后更新**。
- 架构决策以 **ADR**（Architecture Decision Record）形式内嵌于 02，每条含：背景、决策、备选、后果。
- 需求以 **FR-xxx**（功能）/ **NFR-xxx**（非功能）编号，供实施计划（05）与验收引用。
- 调研引用格式 `文件:行号`，基于调研时点的上游 commit（见各调研文档头部）。
