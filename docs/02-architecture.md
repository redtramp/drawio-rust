# 02 · 总体架构

> **状态**：草案 ｜ **最后更新**：2026-10-29 ｜ **依据**：07.1 / 07.2 / 07.3 调研

---

## 2.1 架构总览

```
┌────────────────────────────────────────────────────────────────────────┐
│  Tauri 2 桌面应用 (drawio-rust)                                           │
│                                                                        │
│  ┌────────────────────────── 前端 (Svelte 5) ──────────────────────┐   │
│  │  App Shell（布局/路由）                                          │   │
│  │  ├─ ChatPanel   对话面板（流式消息、工具调用可视化、版本卡）        │   │
│  │  ├─ Canvas      iframe 嵌入 draw.io（public/drawio 同源伺服）        │   │
│  │  ├─ Settings    Provider/模型/API Key 设置                       │   │
│  │  └─ SyncPanel   上游同步入口 + 同步报告                           │   │
│  │  状态：Svelte 5 runes + 全局 store（图状态/版本/会话）             │   │
│  └──────────────▲──────────────────────────────┬──────────────────┘   │
│                 │ postMessage（load/export/…） │ Tauri invoke / events │
│  ┌──────────────┴──────────────────────────────▼──────────────────┐   │
│  │  主进程 (Rust：src-tauri 薄层 + drawio-core 纯逻辑)              │   │
│  │  ├─ ai/          AI 编排：provider 客户端、工具循环、结构校验、   │   │
│  │  │               autoFix、VLM 校验（图片降采样）、重试状态机      │   │
│  │  ├─ doc/         文档服务：.drawio(明文/zip+deflate)、页、diff    │   │
│  │  ├─ visio/       foreignObject→text 转换器（Visio + PDF 复用）    │   │
│  │  ├─ export/      导出编排（iframe SVG/PNG；resvg PDF；Visio 管线）│   │
│  │  └─ settings/    配置 + OS 钥匙串（keyring；Linux 0600 降级）     │   │
│  │  （webapp 由前端 public/drawio 同源伺服；同步 = drawio-sync CLI） │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────────┘
                │ reqwest（OpenAI 兼容 / Anthropic / Google / Ollama …）
                ▼
        各家 LLM API（含 VLM 视觉校验模型）
```

**三层职责划分**：

| 层 | 职责 | 不做什么 |
|----|------|----------|
| Svelte 5 前端 | UI、iframe 桥接（postMessage）、会话/版本状态展示、把 AI 结果注入画布、请求画布截图 | 不直连 LLM、不做结构校验、不读 API Key |
| Rust 主进程 | AI 编排全循环、文档/页/diff、Visio 转换、导出、Key 安全存储、同步 | 不渲染 UI、不直接操作 DOM |
| draw.io webapp（vendored） | 画布渲染、编辑交互、原生导出（SVG/PNG/PDF 生成） | 不做 AI |

## 2.2 技术选型

| 项 | 选择 | 理由 | 备选（已否） |
|----|------|------|--------------|
| 桌面壳 | **Tauri 2** | 用户指定；Rust 主进程 + 系统 WebView，包体小、内存低 | Electron（next-ai-draw-io 用，但重复造轮子、体积大） |
| 后端语言 | **Rust** | 用户指定；纯逻辑（XML 校验/转换/同步）类型安全、可单测、可编译为独立 CLI（sync 工具） | — |
| 前端框架 | **Svelte 5** | 用户指定；runes 模型适合聊天流式状态；产物小 | React（next-ai-draw-io 用，不移植） |
| 编辑器内核 | **draw.io webapp 整包嵌入** | 3.3 万行 EditorUi.js 不可重写；iframe+postMessage 是 draw.io 官方支持的嵌入方式（`?embed=1`），next-ai-draw-io 已验证可行 | 移植 mxGraph 为 Svelte 组件（工作量=重写半个 draw.io，否） |
| LLM 接入 | **reqwest 直连各家端点** | Vercel AI SDK 是 Node/TS 生态，Rust 无对应物；各家端点大多 OpenAI 兼容，reqwest+sse 即可；Provider 差异用数据表抽象 | 内嵌 Node sidecar 跑 AI SDK（多一个运行时，否） |
| XML 处理 | **quick-xml**（+ 必要时 regex 级预处理） | next-ai-draw-io 的 xml-validation 本来就是正则/字符串式解析，语义可平移 | 严格 XML 解析器（draw.io XML 偶有非严格片段，严格解析会误杀） |
| Key 存储 | **keyring crate**（OS 钥匙串） | NFR-1 | 明文文件（否） |
| 构建前端 | **Vite**（Svelte 5 官方模板） | 标准 | — |
| 构建/分发 | **tauri-bundler**（msi/dmg/deb/AppImage） | Tauri 2 内置 | — |

## 2.3 分层与模块图（代码级）

```
drawio-rust/
├─ src-tauri/                      # ★ 单一后端 crate（Tauri 2 主进程 + 纯逻辑）
│  ├─ src/
│  │  ├─ main.rs / lib.rs          # Tauri 入口、命令注册、事件发射
│  │  ├─ error.rs                  # 统一错误类型（Error + Retryable）
│  │  ├─ ai/
│  │  │  ├─ orchestrator.rs        # AI 循环状态机（tool call → 校验 → 重试）
│  │  │  ├─ tools.rs               # 4 个工具 schema + 解析（display/edit/append/get_library）
│  │  │  ├─ prepare.rs             # prepareNewDiagram/Model：包壳、截断检测、样式类展开
│  │  │  ├─ xml_validation.rs      # validateAndFixXml + autoFixXml（~500 行逻辑平移）
│  │  │  ├─ compact_cells.rs       # expandCompactCells（紧凑 mxCell → 完整）
│  │  │  ├─ vlm.rs                 # VLM 视觉校验（含图片降采样）+ ValidationResult
│  │  │  ├─ prompt.rs              # 系统提示词组装（assets/prompt 资源 + canvas context）
│  │  │  ├─ providers/             # provider 数据表 + 端点客户端（openai-compat/anthropic/google）
│  │  │  ├─ session.rs             # 回合内会话上下文
│  │  │  └─ edit.rs / diff.rs      # ID 级编辑 / cell 级 diff
│  │  ├─ doc/
│  │  │  └─ fileio.rs              # .drawio 读写（明文 / zip 容器+raw-deflate 双格式）
│  │  ├─ visio/
│  │  │  └─ converter.rs           # foreignObject→text（port of fix_svg_for_visio.py；PDF 管线复用）
│  │  ├─ session/                  # 版本快照（versions/ + meta.json）
│  │  ├─ sync/                     # 同步逻辑（manifest/fetch/apply/report）
│  │  ├─ export.rs                 # 导出编排（SVG/PNG/Visio 落盘；resvg PDF）
│  │  ├─ settings.rs               # 配置持久化 + keyring 读写（含 0600 文件降级）
│  │  └─ bin/drawio-sync.rs        # 同步 CLI（drawio-sync）入口
│  ├─ assets/prompt/               # 提示词/领域知识（L2 同步目标）
│  │  └─ system-prompt.md  shape-libraries/*.md  model-catalog.json
│  └─ tauri.conf.json
├─ src/                            # Svelte 5 前端（Vite）
│  ├─ main.ts / App.svelte
│  ├─ lib/
│  │  ├─ canvas/DrawioFrame.svelte     # iframe + postMessage 客户端（含同源桥接客户端）
│  │  ├─ canvas/bridge.ts              # embed 协议封装（load/export/autosave）
│  │  ├─ chat/ChatPanel.svelte         # 对话 UI（流式、工具可视化、版本卡）
│  │  ├─ chat/aiClient.ts              # Tauri 命令/事件封装 → 状态
│  │  ├─ settings/SettingsDialog.svelte
│  │  ├─ sync/SyncPanel.svelte
│  │  └─ stores/                        # runes store：diagram/versions/session
│  └─ styles/
├─ public/
│  └─ drawio/                       # ★ vendored draw.io webapp（L1 目标，入库；dev/prod 同源伺服）
├─ index.html / package.json / vite.config.ts
├─ sync/
│  ├─ vendor-keep/patches/         # 本地对 webapp 的有意修改（L1 换版后重放，06 §4.1）
│  └─ reports/                     # 同步报告 <ts>.md
├─ docs/                           # 本工程文档
├─ upstream.toml                   # 同步清单（见 06）
├─ Cargo.toml（workspace）/ package.json
└─ .github/workflows/              # CI：build + 可选 sync-check
```

## 2.4 关键架构决策（ADRs）

### ADR-1：draw.io 集成方式 = iframe 整包嵌入（postMessage 为主，同源直控为增强）

- **背景**：draw.io 编辑器内核（`js/diagramly/EditorUi.js` ~3.3 万行 + `app.min.js` 10MB bundle）功能庞大且纯 JS 无类型，重写或 Svelte 化成本≈重做产品；next-ai-draw-io 已验证 iframe 嵌入可行（`scripts/fetch-drawio.mjs` 下载 webapp → `components/canvas/drawio-frame.tsx` iframe 嵌入）。draw.io 官方支持 `?embed=1` + postMessage 协议（07.1 §5）。
- **决策**：主通道 = **iframe 嵌入 + postMessage embed 协议**（load/save/export/autosave，稳定、官方文档化）。**同源机制**：webapp 放前端 `public/drawio/`，dev（`http://localhost:*`）与 prod（`tauri://localhost`）均与主页面同源——**不用** asset protocol（`asset.localhost` 是独立 origin，会破坏同源直控）。增强通道 = **同源直控桥接**：可访问 iframe 内 `EditorUi` 原型（复刻 editor-bridge.ts 的 `watchForEditorUi`），实现 **AI 改动单步 undo**（`graph.model.execute(new ReplaceDiagram(...))`）、选区读取、变更高亮。
- **备选**：① 移植 mxGraph 源码（115 文件）为 TS/组件——只得到渲染层，编辑器功能（Dialogs/Actions/图标库管理）全部要重做，否；② 调 draw.io 商业 SDK/服务——违背自包含目标，否。
- **后果**：✅ 继承 100% 编辑能力与图标库；✅ 风险低（next-ai-draw-io 同路径生产验证）；⚠️ 同源直控依赖 draw.io 内部 API，上游升级可能破坏 → 由同步机制（06 三级中的「逻辑级」）监控 `EditorUi` 相关行为并回归测试；⚠️ 双 webview（主 + iframe）内存占用需关注（NFR-4 跟踪项）；⚠️ **S2 退出标准含 prod 包同源验证**（见 05 S2，防止 prod 与 dev 行为分叉）。

### ADR-2：AI 编排放在 Rust 主进程（不放 Svelte 前端）

- **背景**：next-ai-draw-io 中 AI 循环分散在 Next.js 服务端（`app/api/chat`）与客户端（`chat-engine.tsx` 驱动工具循环）；其纯逻辑部分（prepare/autoFix/页/diff）已被证明可独立于 DOM（07.2 §8 分类 A）。
- **决策**：AI 全循环（组 prompt → 调 LLM → 解析 tool call → 结构校验/autoFix → 失败回喂重试 → VLM 校验 → 结果事件）在 **Rust** 执行；Svelte 只负责把用户消息发给 Rust、把 Rust 事件渲染出来、把 XML 注入 iframe。
- **理由**：API Key 不出主进程（NFR-1）；纯逻辑在 Rust 可单测、可复用（sync/CLI）；流式转发由 Tauri 事件天然支持；避免在 webview 里维护 2000+ 行 React 状态机。
- **备选**：AI 循环放 Svelte（reqwest 不可用 → fetch 直连，Key 暴露在前端，否）；内嵌 Node sidecar（多运行时，否）。
- **后果**：✅ 安全/可测；⚠️ Rust 重写 autoFixXml（~500 行正则/字符串逻辑）需要逐条用例对齐（05 计划 Step 8 的验收标准：对 20 组已知 XML 用例输出与 TS 版一致）。

### ADR-3：draw.io 资源来源 = 从 drawio 上游仓库 vendor webapp（而非下载 draw.war）

- **背景**：next-ai-draw-io 按版本+sha256 下载 draw.war（07.2 §4）；但本项目已有 drawio 上游 git 仓库（`sample/drawio`，branch `dev`，含完整 `src/main/webapp/` 且 `.min.js` 已提交，07.1 §2/§5）。
- **决策**：同步工具把 `drawio:src/main/webapp/*` **整拷** 到 `drawio-rust/public/drawio/`（入库；Vite 自动伺服 public/ 静态 → **dev 与 prod 均与主页面同源**；asset protocol 仅用于会话缩略图等小资源）。iframe `src="/drawio/index.html?embed=1&proto=json&configure=1&ui=simple"`。pinned commit 记录于 `upstream.toml`。
- **理由**：与「跟踪上游」目标天然一致（git diff 直接给出变更）；draw.war 无 git 历史、版本粒度粗；仓库 webapp 已含 min 产物可直接伺服。
- **后果**：✅ 同步=git 操作，可审计；✅ dev/prod 同源均成立（S6 同源桥接的地基）；⚠️ `public/drawio/` 体积大（~20MB 级）→ 默认**直接入库**保证自包含（备选 git 子模块/LFS 见 06 §4.1）。

### ADR-4：Visio 导出 = Python 脚本逐逻辑译为 Rust

- **背景**：drawio-to-visio 全部功能 = 一个 232 行 Python 脚本（lxml 解析 SVG，`<switch>` 内 `foreignObject` → `<text>`，07.3）。
- **决策**：Rust `visio/converter.rs` 用 quick-xml 实现同算法（保留 `light-dark()` 颜色处理、锚点/坐标计算、`dominant-baseline="hanging"`）；管线 = draw.io 导出 SVG → converter → 落盘 `.svg`。
- **备选**：① 打包 Python 解释器/脚本运行时——桌面应用带 Python 依赖不可接受，否；② 保留 Python 脚本随包分发+子进程调用——破坏「无外部依赖」且跨平台麻烦，否（列为降级备胎）。
- **后果**：✅ 零外部依赖；⚠️ 上游已知限制继承（多行/自动换行文本不完美，07.3）→ Q3 的 PNG 底图降级方案留 v1.x。

### ADR-5：前端 UI = Svelte 5 全新构建（不移植 React 代码）

- **背景**：next-ai-draw-io UI 为 React 19 + Radix + 2000+ 行 chat-engine（07.2 §8 分类 C），与 Svelte 5 无共享价值。
- **决策**：UI 全部用 Svelte 5 重写；从上游继承的是**交互设计与领域知识**（消息结构、版本卡、工具可视化、样式规则），不是代码。
- **后果**：✅ 技术栈纯净；⚠️ UI 工作量不可省（05 计划按 ChatPanel/Canvas/Settings/Sync 四块拆步）。

### ADR-6：上游同步 = 三级策略（vendor 整拷 / 文本资源 / 逻辑报告）+ Rust CLI 工具

- **背景**：drawio-rust 是再架构不是拷贝——三类上游内容的同步可行性完全不同（07.2 §8：A 纯逻辑已译 Rust、B 文本资源原样搬、C 浏览器代码不搬）。
- **决策**：
  - **L1 vendor**：drawio webapp 整目录，git 级换版（pinned commit → latest，整拷+记录）；
  - **L2 文本**：提示词、shape-libraries/*.md、model-catalog.json 等「原样搬」资产，逐文件同步（hash 比对，变更即覆盖并提示）；
  - **L3 逻辑**：上游 TS/Python 纯逻辑（xml-validation、edit-diagram、fix_svg…）变更 → **不自动合并**，生成「再移植报告」（变更文件/摘要/影响的 Rust 模块/建议），人工评审后移植。
  - 载体：`drawio-sync`（Rust bin，`cargo run --bin drawio-sync`，亦可独立编译分发）。
- **详见**：[06 上游同步机制](06-upstream-sync.md)。

### ADR-7：LLM 接入 = reqwest 直连 + 数据表驱动 Provider 抽象

- **背景**：next-ai-draw-io 的 25 家 provider 抽象建在 Vercel AI SDK 之上（07.2 §6）；Rust 无该 SDK。
- **决策**：Rust `ai/providers/` 用**数据表**（provider 元数据：端点模板、鉴权方式、sampling 参数映射、模型目录）+ 少量协议客户端（`openai-compat`（覆盖 openai/deepseek/qwen/ollama/sglang/自定义…）、`anthropic`、`google`）实现 MVP 10+ 家；`model-catalog.json`（models.dev 数据）直接复用做提示与能力查询。
- **后果**：✅ 依赖极小；⚠️ 新 provider = 加一行数据表（若 OpenAI 兼容）或小客户端（协议特殊时），长期与上游 provider 表对齐（L2/L3 同步覆盖）。

### ADR-8：文件与状态持久化

- **决策**：`.drawio` 即源文件（无自有 DB）；会话/版本历史存于应用数据目录（`%APPDATA%/drawio-rust/sessions/<id>/`：snapshots + meta.json），版本策略对齐 next-ai-draw-io（每页快照、上限 20 份，07.2 §5/§7）。
- **后果**：✅ 无 schema 迁移负担；版本可清空。

### ADR-9：PDF 导出 = 纯 Rust resvg 跨平台主路径（复用 Visio 转换器）+ Windows WebView2 高保真选项

- **背景**：Tauri 2 无跨平台 print-to-PDF 公开 API（macOS WKWebView / Linux WebKitGTK 均未暴露；上游 drawio 的 PDF 打印在闭源 Electron 主进程中，07.1 §4）。「隐藏 webview + export3.html 截窗/打印」在 mac/linux 无对应 API，且 CI 无法验证。
- **决策**：
  - **主路径（跨平台，v0.1）**：iframe 导出 SVG → **复用 `visio::converter` 将 foreignObject 替换为 `<text>`**（resvg 不支持 foreignObject，不复用则文字丢失）→ `resvg` 光栅化 → `printpdf` 分页生成 PDF。纯 Rust、可 headless 运行、CI 可验证。
  - **选项（Windows，v1.x）**：WebView2 `ICoreWebView2::TryPrintToPdf` 矢量高保真 PDF（真实 webview 打印，文字可选取），无需 converter。
- **备选（已否）**：① 捆绑 wkhtmltopdf / headless Chromium——外部二进制依赖，否；② WebKit 私有 API——生产不可用，否。
- **后果**：✅ v0.1 PDF 可落地且 CI 可验证；⚠️ 主路径为**光栅 PDF**（文字不可选取、页面参数简化）→ UI 明示「矢量高保真请导出 SVG」；Windows 高保真路径列入 v1.x。

## 2.5 核心数据流

### 2.5.1 AI 生成（display_diagram）

```
用户输入 → ChatPanel → invoke("ai_chat_send", {message})
Rust: 取画布XML（invoke 由前端在发起前 postMessage 采集，或 ai_chat_send 内部经 event 询问）
     → 组装 system prompt + canvas context + tools → provider.stream()
     → 流式：token 事件 → 前端打字机；tool_call(display_diagram, {xml})
     → prepare + validate + autoFix（Rust）
        ├─ 失败 → 回喂 error → 再请求（≤N 次）
        └─ 成功 → event("ai_tool_result", {tool, xml, op})
前端: Canvas.postMessage("load", {xml})（或同源 commit 单步 undo）
     → 请求 iframe 导出 PNG → 发 Rust
Rust: VLM 校验 → 通过→event("ai_turn_done")；有问题→回喂 LLM 重试（≤3 次，末次放行）
```

### 2.5.2 常规导出（含 Visio）

```
用户「导出 Visio SVG」→ invoke("export_file", {format:"visio-svg", path})
Rust: 向 iframe postMessage("export", {format:"svg"}) → 收 SVG dataURI
     → visio::converter::fix(svg) → 写盘
前端: 弹「已导出」+ 打开位置
```

### 2.5.3 打开/保存

```
invoke("file_open") → dialog 选 .drawio → doc::fileio 读（deflate 检测解压）
     → event 发前端 → postMessage("load")
「保存」→ postMessage 取当前 XML（autosave 通道已有）→ doc::fileio 写
```

### 2.5.4 上游同步

```
SyncPanel「立即同步」→ invoke("sync_run") → sync::fetch（git fetch 各上游）
→ 对每个 pinned commit..HEAD：
   L1：整拷 vendor（drawio webapp）
   L2：hash 比对文本资源，变更覆盖 + 记录
   L3：git diff 摘要 → 匹配已知「逻辑映射表」（上游文件→Rust 模块）→ 生成报告
→ 更新 upstream.toml 的 pinned commit → event("sync_done", {report})
```

## 2.6 横切关注点

- **安全**：API Key 仅 keyring（NFR-1；Linux 无 Secret Service 时显式降级 0600 文件 + UI 警示，03.5 §4）；LLM 请求仅经 Rust 发出；iframe 与主 UI 同源（dev `http://localhost:*` / prod `tauri://localhost`，webapp 在前端 public/ 内，见 ADR-1），postMessage 仍严格校验 `event.origin` 双保险（白名单见 04 §3）；SSRF 防护（自定义端点禁内网/元数据地址，移植 `lib/ssrf-protection.ts` 规则；**Ollama localhost 默认端口为显式例外**，03.5 §4）。
- **错误处理**：Rust 统一 `thiserror` 错误类型 + 结构化日志（`tracing`）；AI 循环错误分「可重试（回喂）」与「致命（上报 UI）」两类。
- **可测试**：纯逻辑（prepare/xmlfix/compact/pages/diff/visio/sync-manifest）100% 单测；上游用例对齐（见 ADR-2）；postMessage 协议用协议级集成测试（headless webview 或 mock）。

## 2.7 许可与合规（对应 NFR-3）

| 来源 | 许可 | 义务 |
|------|------|------|
| draw.io webapp（vendored） | Apache-2.0 | 保留 `LICENSE`/`NOTICE` 于 vendor 目录与安装包；修改文件加声明；不得声称官方关联 |
| next-ai-draw-io 提示词/资产 | Apache-2.0 | 同上（assets/prompt 保留来源文件头） |
| drawio-to-visio 脚本（译 Rust） | 见其 LICENSE | 来源注释 |
| libavoid-js（draw.io 内） | LGPL-2.1 | 随 vendor 目录原样分发即可（未修改） |
