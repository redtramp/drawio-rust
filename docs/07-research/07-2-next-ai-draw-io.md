# 07.2 · 调研：next-ai-draw-io（DayuanJiang/next-ai-draw-io）

> **状态**：已完成 ｜ **调研时点**：2026-10-29 ｜ **上游**：`github.com/DayuanJiang/next-ai-draw-io` branch `main` @ `05adbe5`（v0.5.0，Apache-2.0）

---

## 1. 项目形态

Next.js 16 + React 19 + Vercel AI SDK v6 + Electron 的「AI 对话画 draw.io」应用（Web 站 + 桌面 + MCP Server 三形态，579 个跟踪文件）。`predev/prebuild` 跑 `scripts/fetch-drawio.mjs` 固定版本+sha256 下载 draw.war 解包到 `public/drawio` 供 iframe 嵌入。

## 2. AI 工具调用协议（`lib/chat-tools.ts:19-135`）

Vercel AI SDK 结构化 JSON 工具调用（`tool()`），**不是裸 XML 也不是 diff**；AI 输出「紧凑 mxCell 片段」由程序补全。4 个工具：

| 工具 | 入参 | 说明 |
|------|------|------|
| `display_diagram` | `{ xml: string }` | 只传 `<mxStyle>` 定义 + 兄弟级 `<mxCell>`（shape 自闭合带 x,y,w,h；edge 带 source,target）；外壳（mxfile/mxGraphModel/root、root cells "0"/"1"、vertex/edge/parent="1"/mxGeometry）自动补（:21-46）。浏览器端执行，无 server execute |
| `edit_diagram` | `{ operations: [{ operation: update\|add\|delete, cell_id, new_xml? }] }` | ID 级指令集；delete 自动级联子 cell 与边（:48-90） |
| `append_diagram` | `{ xml: string }` | display_diagram 输出截断后续写（:91-111） |
| `get_shape_library` | `{ library: string }` | 唯一服务端执行，读 `docs/shape-libraries/*.md`（30+ 图标库，:112-134） |

## 3. 系统提示词（`lib/system-prompts.ts`）

仅一个 `getSystemPrompt(modelId, minimalStyle)`（:253-268）= `DEFAULT_SYSTEM_PROMPT`（:12-206）+ `STYLE_INSTRUCTIONS`（:211）或前缀 `MINIMAL_STYLE_INSTRUCTION`（:223）。

结构：角色 → App 上下文/功能 → 工具说明 → **画布状态协议**（「[Canvas when this message was sent...]」，与 `lib/diagram-context.ts` 配合，**只在画布与模型已知状态有差异时**随消息发 XML，:1-19）→ 工具选择规则 → 布局约束（x 0-800 / y 0-600）→ XML 结构参考 → **8 条边路由规则**（waypoints / exit-entry / 障碍绕行，:152-204）。

硬编码领域知识：mxGraphModel/mxCell 结构、style 语法、共享样式类规则；**shape id 不在 prompt 里**，靠 `get_shape_library` 动态查（:104 强制「never guess」）。示例/样式规则与 MCP 共享：`packages/mcp-server/src/xml-examples.ts`。

## 4. 校验管线（两层）

**结构层（纯逻辑，web 与 MCP 共用）**：
`prepareNewDiagram`（mcp-server/src/new-diagram.ts:192）→ `prepareNewModel`（:161）：取 mxStyle 类 → `reservedIdError`（禁 "0"/"1"）→ 截断检测 `isMxCellXmlComplete`（:71）→ 包 `<mxGraphModel>` → `validateAndFixXml`（xml-validation.ts:978，含 ~500 行 `autoFixXml` :449：引号修复、重复 id/属性、嵌套 mxCell、orphan mxPoint 检查）→ `expandCompactCells`（紧凑→完整 cell）→ 展开样式类 → 补 html=1/whiteSpace=wrap。

**视觉层（VLM）**：commit 后等 100ms 截 PNG → POST `/api/validate-diagram`（app/api/validate-diagram/route.ts:127-156）：`generateText` + `Output.object({ schema: ValidationResultSchema })`（validation-schema.ts:9-35：valid / issues[type,severity] / suggestions）+ `VALIDATION_SYSTEM_PROMPT`（validation-prompts.ts:6，列 6 类视觉问题）。

**反馈/重试**：失败时 `addToolOutput({ state: "output-error", errorText: formatValidationFeedback(...) })`（diagram-validator.ts:18-58 生成 Critical/Warnings/Suggestions 文本）→ 客户端 `sendAutomaticallyWhen`（chat-engine.tsx:648-652）自动再请求 → 模型重试；VLM 最多 3 次（MAX_VALIDATION_RETRIES，use-diagram-tool-handlers.ts:34,336），末次带警告放行；**VLM 服务故障默认 valid**（不阻塞用户，route.ts:172）。

## 5. 编辑器桥接（`lib/drawio/`）

画布 = **iframe 内嵌完整 draw.io web app**（非 CDN、非 JS 库）：
- `scripts/fetch-drawio.mjs`（predev/prebuild）按 drawio-version.json 固定版本+sha256 下载 draw.war（zip），node:zlib 解到 `public/drawio`（:13-14,57-60）；**同源托管**是为了能直接调 draw.io 内部 JS（:4-7），跨源只走 postMessage；
- URL 由 `getDrawioSrc` 拼（drawio-config.ts:46-73：`embed=1`、`proto=json`、`configure=1`、`ui=simple`）；
- 注入双通道（components/canvas/drawio-frame.tsx）：
  1. postMessage embed 协议 `send("load", { xml })`（:80-90）；
  2. 同源时 `watchForEditorUi` hook 住 `EditorUi.prototype.updateActionStates` 拿到 editor 对象（editor-bridge.ts:1037-1064），之后直接操作：
     - `commitDiagram` 用 `graph.model.execute(new win.ReplaceDiagram(...))` 作为**单步 undo** 提交（:473-509）；流式预览走 preview 不记 undo（:213-215,424）；
     - 还能读选区（`readSelectionDetails`:835）、高亮变更（`highlightCells`:738）、导出、暗色模式。

## 6. 版本/差异

- `lib/diagram-diff.ts`：`diffDiagrams`（:159-191）按页按 id 做 cell 级 diff（signature = 规范化 style+geometry，:86），输出增/删/改计数 + touchedIds + fromScratch → 驱动变更高亮与「用户手改检测」；`isSameDocument`（:359）比较其他页；
- `lib/diagram-pages.ts`：页管理纯函数——`pageModelXml`（:89）抽页、`replacePageModel`（:109）、`placeOnPage`（:141）：AI 画的单页写回画布文档，保留其他页和 file vars；
- 版本历史在客户端 zustand（canvas/versions-context、use-versions）；MCP 侧 history.ts 留 20 份快照 + restore_version 工具。

## 7. 模型接入

是 Vercel AI SDK：服务端 `streamText`（app/api/chat/route.ts:471，tools 在 :550）→ SSE UI-message 流（withHeartbeat 保活）；客户端 `@ai-sdk/react` 的 `useChat`（chat-engine.tsx:3,520），`experimental_throttle:150`（:647）。**多步工具循环由客户端** addToolOutput + sendAutomaticallyWhen 驱动（:648），非服务端 maxSteps。

- `lib/providers.ts`：**纯数据表**，25 个 provider（:105-547：openai/anthropic/google/deepseek/qwen/doubao/glm/kimi/minimax/mimo/openrouter/aihubmix/siliconflow/modelscope/novita/atlascloud/qiniu/gateway/bedrock/vertexai/azure/edgeone/ollama/sglang/custom），每条含 `Sdk`（:12-26，映射到 @ai-sdk/* 工厂）、凭证类型、env 前缀、默认 URL、模型列表风格；
- `lib/ai-providers.ts`：`getAIModel`（:1083）解析密钥/端点 → `buildProviderOptions`（:306，各家 sampling/thinking）→ `createModel`（:975）调对应工厂；`getValidationModel`（:1391）供 VLM；
- `lib/model-catalog.ts` + `model-catalog.json`：models.dev 数据（tools/images/reasoning、上下文、价格），仅用于提示与成本估算（:15-19 注释明确「requests are sent the same way either way」）。

## 8. MCP Server（`packages/mcp-server`）

16 个工具（src/index.ts）：`get_drawing_guide`（:391）、`get_shape_library`（:413）、`start_session`（:449）、`list_saved_diagrams`（:531）、`create_new_diagram`（:577, {xml, screenshot?}）、`load_diagram`（:679, {path | xml}）、`edit_diagram`（:784，与 web 同 schema）、`get_diagram`（:964）、`get_selection`（:1060）、`screenshot_diagram`（:1302）、`export_diagram`（:1328）、`list_pages`/`add_page`/`rename_page`/`delete_page`（:1639-1780）、`restore_version`（:1871）。

**共享核心**：XML 管线代码（new-diagram、edit-diagram、compact-cells、embedded-images、pages、xml-validation）被主应用直接 import（chat-tools.ts:7-11、use-diagram-tool-handlers.ts:15-19）——同一套纯逻辑，两处复用。MCP 特有：http-server.ts（Node 状态协议：预览页 2s 轮询 GET /api/state、用户编辑带版本推送、冲突 409，shell/mcp-sync-core.ts:8-15）、history/persistence、浏览器内导出截图（exportViaBrowser → captureScreenshot:1227）。MCP 服务本身**无 LLM**，由外部 MCP 客户端驱动。

## 9. 可移植性分类（对 drawio-rust 的核心结论）

**A. 纯逻辑（直接译 Rust，放后端）**
- mcp-server/src：`new-diagram.ts`、`xml-validation.ts`（正则/字符串式解析，无 DOM 依赖）、`compact-cells.ts`、`style-classes.ts`、`xml-attributes.ts`、`edit-diagram.ts`、`pages.ts`（含 deflate 解压）、`embedded-images.ts`、`shape-library.ts`（改读本地文件）、`selection.ts`
- lib：`diagram-diff.ts`、`diagram-pages.ts`（DOMParser/XMLSerializer 换成 quick-xml，逻辑不变）、`diagram-validator.ts`（zod→schemars）、`validation-schema.ts`、`drawio-config.ts`（URL 拼装）
- 校验/重试循环逻辑（use-diagram-tool-handlers.ts 中 prepare→commit→VLM→retry 状态机可整体后端化）

**B. 提示词/领域知识（原样搬文本）**
- `system-prompts.ts`、`validation-prompts.ts`、`drawing-guide.ts`、`xml-examples.ts`、`docs/shape-libraries/*.md`（30 个 md）、`model-catalog.json`

**C. 强依赖浏览器/React（Tauri 侧保留 web 或重实现）**
- `editor-bridge.ts`（深度 hook draw.io 内部 API：ReplaceDiagram/undoManager/highlight/selection）——Tauri 下把 draw.io 同域内嵌进 webview 即可复刻（同源成立）；最低成本是只用 postMessage（load/export/autosave），放弃同源直控（= 内建降级路径）
- `drawio-frame.tsx`、`chat-engine.tsx`（2014 行）、`use-diagram-streaming.ts`（流式预览）、全部 UI 组件
- `fetch-drawio.mjs`（Node 构建脚本 → 由 drawio-sync 的 L1 vendor 机制替代）

**D. 架构提示（已被 02 ADR 采纳）**
- Rust 后端接管：工具 schema、prepare/validate/autofix、edit 操作、页管理、diff、VLM 校验调用、provider 抽象（reqwest 直接打各家 OpenAI/Anthropic 兼容端点，**无需 AI SDK**）；
- 截图给 VLM 的路径改为 Tauri 端从 draw.io `export` 消息取 PNG 回传，替代前端 canvas 截屏（use-diagram-tool-handlers.ts:308）。
