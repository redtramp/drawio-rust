# 01 · 需求分析

> **状态**：草案 ｜ **最后更新**：2026-10-29 ｜ **依据**：07.1 / 07.2 / 07.3 调研

---

## 1.1 背景与动机

现有三个上游项目各自解决一部分问题，但彼此割裂：

- **drawio**：功能完整的图形编辑器，但**没有 AI 能力**，导出 Visio 时文本错位。
- **next-ai-draw-io**：能用 AI 对话生成/编辑 draw.io 图形，但它是 **Web/Electron（React）** 应用，编辑器以 iframe 方式内嵌 draw.io，且**不含 Visio 导出**。
- **drawio-to-visio**：仅一个 Python 脚本 + 技能说明，解决 **draw.io SVG → Visio 兼容**（foreignObject 文本错位），无产品形态。

**动机**：构建一个统一的桌面应用 **drawio-rust**，让用户在一个应用里「**用 AI 画 → 手工编辑 → 导出（含 Visio 兼容）**」，且能**持续跟踪三个上游的更新**保持能力同步。

## 1.2 目标（Goals）

| # | 目标 | 说明 |
|---|------|------|
| G1 | AI 对话生成图形 | 一句话/上传文件 → 在真实 draw.io 画布上生成图形（含 AWS/Azure/GCP/K8s 图标库） |
| G2 | AI 对话编辑图形 | 选中若干形状后对话，AI 只改选中部分；每次 AI 改动是可对比/可回滚的版本 |
| G3 | 完整 draw.io 编辑能力 | 继承 draw.io 原生编辑、多页、样式、图标库（不重写内核） |
| G4 | Visio 兼容导出 | 一键导出 Visio 可正确编辑的 SVG（文本位置正确、可编辑） |
| G5 | 常规导出 | `.drawio` / `.png` / `.svg` / `.pdf` |
| G6 | 多模型 Provider | 支持主流 LLM Provider（含 OpenAI 兼容自定义端点、本地 Ollama），API Key 本地保存 |
| G7 | 上游同步 | 可持续跟踪三个上游更新，按机制同步到 drawio-rust |

## 1.3 非目标（Non-Goals，本期不做）

- ❌ 重写 draw.io 编辑器内核（`EditorUi.js` 3.3 万行）——整包嵌入，不重写。
- ❌ 移植 next-ai-draw-io 的 **React UI / 在线 Web 服务 / 多租户 / 配额计费 / 管理后台 / 画廊 / 多语言 SEO**——只取 AI 能力，UI 用 Svelte 5 重建。
- ❌ 复刻 next-ai-draw-io 的 **MCP Server** 对外服务（列为 v1.0 后的可选扩展，见 §1.7）。
- ❌ 复刻 **在线 Web 部署**（Cloudflare open-next）——drawio-rust 是桌面应用。
- ❌ 实时协同编辑 / 云端同步存储。

## 1.4 用户与场景

- **用户画像**：需要频繁画图（架构、流程、ER、时序）且希望用 AI 提效的技术人员/架构师；部分需把图交付给用 Visio 的同事。
- **核心场景**：
  1. 打开应用 → 输入「画一个高可用 AWS 架构」→ AI 在画布生成 → 手工微调 → 导出 `.drawio`。
  2. 选中画布上 3 个形状 → 对话「把这三个改成红色并加注释」→ AI 只改这三处 → 对比/回滚。
  3. 生成/编辑完成 → 「导出 Visio SVG」→ 在 Visio 中打开文本位置正确。
  4. 上游 next-ai-draw-io 发布了新的图标库/提示词改进 → 运行同步 → drawio-rust 获得更新。

## 1.5 功能需求（FR）

> 「阶段」列 = 交付版本，与 §1.8 分期严格一致（P0=v0.1 … P4=v0.5）。

### 编辑器
| ID | 需求 | 阶段 |
|----|------|------|
| FR-E1 | 内嵌 draw.io 画布，支持完整原生编辑（拖拽、样式、连线、图标库） | v0.1 |
| FR-E2 | 多页文档支持（页管理、跨页） | v0.1 |
| FR-E3 | 打开/保存 `.drawio` 文件（含压缩/未压缩格式读写） | v0.1 |
| FR-E4 | 撤销/重做（draw.io 原生）；**AI 改动作为单步 undo**（同源桥接，Q1；桥接前降级=版本恢复） | v0.1 原生 / v0.3 单步 |
| FR-E5 | 读取当前画布 XML 供 AI 上下文（canvas context snapshot） | v0.2 |
| FR-E6 | 选中形状读取（selection details）供「改选中」 | v0.3 |

### AI
| ID | 需求 | 阶段 |
|----|------|------|
| FR-A1 | 对话生成图形（工具 `display_diagram`/`append_diagram`） | v0.2 |
| FR-A2 | 对话编辑图形（工具 `edit_diagram`，ID 级增/删/改，级联子 cell 与边） | v0.3 |
| FR-A3 | 查询图形库（工具 `get_shape_library`，30+ 图标库 md） | v0.2 |
| FR-A4 | 结构校验 + 自动修复（XML 完整性、保留 id、截断检测、样式类展开） | v0.2 |
| FR-A5 | 校验失败自动反馈给 LLM 并重试（含 VLM 视觉校验，≤3 次，末次放行） | v0.3 |
| FR-A6 | AI 改动版本化（版本卡、缩略图、对比、恢复、undo） | v0.3 |
| FR-A7 | 上传文件（截图/PDF/Markdown/代码）→ 生成对应图形 | v1.0 |
| FR-A8 | 流式输出，AI 改动过程在画布上流式预览并高亮 | v0.3 |
| FR-A9 | 多 Provider（≥ 主流 10 家 + OpenAI 兼容自定义 + Ollama 本地） | v0.2 |
| FR-A10 | 思考型模型展示 thinking 过程 | v1.0 |

### 导出
| ID | 需求 | 阶段 |
|----|------|------|
| FR-X1 | 导出 `.drawio` / `.png` / `.svg` / `.pdf`（pdf：resvg 跨平台路径为主，02 ADR-9） | v0.1 |
| FR-X2 | **Visio 兼容 SVG 导出**（foreignObject→text，文本位置正确可编辑） | v0.4 |
| FR-X3 | 导出分辨率/缩放/边距等选项 | v0.1 |

### 应用
| ID | 需求 | 阶段 |
|----|------|------|
| FR-S1 | Provider/模型设置（API Key 存 OS 钥匙串；Linux 无 Secret Service 时降级 0600 文件+警示，03.5 §4） | v0.2 最小 / v0.5 全量 |
| FR-S2 | 上游同步入口（触发同步、查看同步报告） | v0.5 |
| FR-S3 | 界面国际化（至少中/英） | v0.5 |
| FR-S4 | 深色模式 | v0.5 |

## 1.6 非功能需求（NFR）

| ID | 需求 | 指标/说明 |
|----|------|-----------|
| NFR-1 | 安全：API Key 仅存 OS 钥匙串（keyring crate），不写入明文配置；机制自 v0.2（S7）生效；Linux 无 Secret Service 时显式降级 0600 文件 + UI 警示（03.5 §4） | P0 |
| NFR-2 | 离线可用：无网时可打开/编辑/导出；仅 AI 功能需网 | P0 |
| NFR-3 | 许可合规：draw.io 为 **Apache-2.0**，需保留其版权/NOTICE；libavoid-js 为 LGPL-2.1；产品**不得**声称与 draw.io 官方关联（商标不授予） | P0 |
| NFR-4 | 性能：画布交互帧率不低于原生 draw.io；AI 流式首 token < 3s（取决于模型） | P1 |
| NFR-5 | 可维护性：纯逻辑与 UI 分离；上游同步自动化率最大化 | P1 |
| NFR-6 | 平台：Windows / macOS / Linux 桌面 | P0 |
| NFR-7 | 可观测性：AI 调用/校验/重试留日志，便于排查 | P2 |

## 1.7 约束（Constraints）

1. **技术栈强制**：Rust + Tauri 2（后端/桌面）+ Svelte 5（前端）。不可更换。
2. **draw.io 形态**：上游 `sample/drawio` 是**纯 JS、无运行时构建**的 webapp（`app.min.js`/`viewer.min.js` 为已提交的 Closure 编译产物）；**Electron 主进程源码不公开**（官方仅发二进制）。→ 我们**只 vendor 其 webapp 静态资源**，桌面壳由 Tauri 自建（见 02 ADR-1/ADR-3）。
3. **许可**：draw.io Apache-2.0（可自由修改/再分发，需保留许可与 NOTICE）；next-ai-draw-io Apache-2.0；drawio-to-visio 见其 LICENSE。
4. **AI 能力来源**：next-ai-draw-io 的 AI 管线为 TypeScript（部分强依赖 React/浏览器）。→ 纯逻辑译 Rust，提示词原样搬，UI 重建（见 07.2 可移植性分类 A/B/C）。
5. **上游为独立 git 仓库**（各有 GitHub remote），drawio-rust 为独立项目，通过同步机制消费三者（见 06）。

## 1.8 分期范围（Phased Scope）

| 阶段 | 版本 | 范围 | 退出标准 |
|------|------|------|----------|
| **P0 · MVP** | v0.1 | 画布嵌入 + 打开/保存 + 常规导出（FR-E1/E2/E3/X1/X3、FR-E4 原生撤销重做） | 能像 draw.io 一样打开、编辑、存 `.drawio`、导出 png/svg/xml/pdf |
| **P1 · AI 生成** | v0.2 | AI 对话生成 + 图标库 + 结构校验 + 最小设置（FR-A1/A3/A4/A9、FR-E5、FR-S1 最小） | 一句话生成图形上画布，校验通过 |
| **P2 · AI 编辑** | v0.3 | AI 编辑 + 版本/undo + VLM 重试 + 流式高亮（FR-A2/A5/A6/A8、FR-E4 单步 undo、FR-E6） | 改选中、对比/回滚、失败自动重试 |
| **P3 · Visio** | v0.4 | Visio 兼容导出（FR-X2） | 导出 SVG 在 Visio 中文本位置正确 |
| **P4 · 同步与打磨** | v0.5 | 上游同步工具（FR-S2、NFR-5）+ i18n/深色（FR-S3/S4） | 运行同步能拉取上游更新并出报告 |
| **v1.0** | — | 文件上传生成（FR-A7）、thinking（FR-A10）、多 Provider 全量、平台打包签名 | 稳定可用 |
| 可选扩展 | v1.x | MCP Server、在线 Web 形态 | 视需求 |

> **决策点（需用户确认）**：MVP（v0.1）是否从「画布 + 文件 + 导出」起步，还是希望**先做 AI 生成**？默认按上表 P0→P1 顺序（先有可用画布，再上 AI），见 [05 实施计划](05-implementation-plan.md)。

## 1.9 开放问题（Open Questions）

| # | 问题 | 当前倾向 |
|---|------|----------|
| Q1 | draw.io 画布同源直控（ReplaceDiagram 单步 undo）是否 P1 就做，还是先用 postMessage（无单步 undo）？ | 倾向 P2 做同源桥接，P0/P1 用 postMessage |
| Q2 | LLM 调用是否经 Rust 直连各家端点（reqwest），不引 Vercel AI SDK？ | 是，reqwest 直连（02 ADR-7） |
| Q3 | Visio 导出是否也支持「PNG 底图 + 文本」降级方案（上游已知限制）？ | 列为 P3 增强项 |
| Q4 | 是否需要把同步做成 CI 定时任务，还是仅手动命令？ | 手动命令为主 + 可选 CI cron（06） |
