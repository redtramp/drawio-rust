# 03.2 · 编辑器集成设计（draw.io 内核嵌入）

> **状态**：草案 ｜ **最后更新**：2026-10-29 ｜ **上游依据**：07.1（drawio 调研）、07.2 §4（next-ai-draw-io 桥接调研）

---

## 1. 模块职责

把 draw.io webapp 以 iframe 嵌入 Tauri 主 webview，提供：加载/保存文档、导出（SVG/PNG/PDF）、画布 XML 快照、选区读取；并为 AI 改动提供**单步 undo 提交**与**变更高亮**（同源桥接，P2 引入）。对应 FR-E1~E6、FR-X1。

## 2. 资源伺服（public/drawio → /drawio/）

- 同步工具 L1 目标 = `public/drawio/`（= 上游 `src/main/webapp/*` 整拷，见 06 §4.1）；本地有意修改放 `sync/vendor-keep/patches/`（换版后重放）。
- Vite 自动伺服 `public/` 静态：dev `http://localhost:<port>/drawio/`、prod `tauri://localhost/drawio/`，**与主页面同源**（同源桥接的地基，ADR-1/ADR-3）；asset protocol 仅用于会话缩略图等小资源，**不**伺服 webapp。
- 关键产物（07.1 §2）：`index.html` → `js/bootstrap.js` → `mxgraph/mxClient.js`（min 单文件）+ `js/app.min.js`（10MB，已提交 Closure 产物）；`styles/grapheditor.css`。**无运行时构建，直接伺服即可**。
- 伺服约束：保持相对路径完整（webapp 内部全用相对引用）；`Content-Type` 正确（.js→javascript）；缓存策略 dev 关缓存。

## 3. iframe 嵌入参数

```
/drawio/index.html?embed=1&proto=json&configure=1&ui=simple
  &lang=zh-CN&spin=1&saveAndExit=0
```

- `embed=1`：官方嵌入模式（隐藏主菜单外框，postMessage 协议开启，07.1 §5 `viewer.min.js:18,30`）；
- `proto=json`：消息 JSON 序列化（对齐 next-ai-draw-io `drawio-config.ts:46-73` 的 getDrawioSrc）；
- `configure=1`：允许宿主经 postMessage 配置；
- `ui=simple`：简化 UI（减少与宿主 Chrome 的重复）。

## 4. postMessage 协议（主通道，P0 起）

前端 `lib/canvas/bridge.ts` 封装；**双向**，均校验 `event.origin === tauri://localhost`（dev 为 http://localhost:1420，白名单常量）。

### 宿主 → drawio（`contentWindow.postMessage(JSON, "*")`）

| action | payload | 用途 |
|--------|---------|------|
| `load` | `{ xml: string, name?: string }` | 加载文档（AI 生成/编辑结果、打开文件、版本恢复） |
| `export` | `{ format: "svg"|"png"|"xml"|"pdf", scale?, border?, background? }` | 请求导出，结果经 `export` 事件回传（dataURI） |
| `autosave` | `{ intervalMs }` | 开启自动保存，drawio 侧定期发 `save` 消息 |
| `configure` | `{ theme?: "dark"|"light" }` | 暗色模式（FR-S4） |
| `getSelection` | `{}` | 请求选区（P1，经同源桥接时可不走此通道） |

### drawio → 宿主（`window.addEventListener("message")`）

| action | payload | 用途 |
|--------|---------|------|
| `init` | `{ version }` | 就绪信号（bridge 等待后才允许 load） |
| `save` | `{ xml, name, autorouted }` | autosave/保存内容回传 |
| `export` | `{ data: dataURI, format }` | 导出结果 |
| `error` | `{ message }` | 加载/导出失败 |
| `message` | `{ text }` | 一般通知（转前端 toast） |

> 协议以上游 draw.io 嵌入文档与 next-ai-draw-io 实际使用集（drawio-frame.tsx:80-90）为准；04 文档给出逐条 TypeScript 接口与 Rust 事件映射。

## 5. 同源桥接（增强通道，P2 引入，FR-E4 单步 undo 依赖）

Tauri 下主 webview 与 iframe **同源**（`tauri://localhost`），因此可复刻 next-ai-draw-io 的 `watchForEditorUi` 直控（07.2 §4，editor-bridge.ts:1037-1064）：

1. **挂接**：轮询 iframe `contentWindow`，hook `EditorUi.prototype.updateActionStates`，在其首次执行时捕获 `this`（editorUi 实例）→ 拿到 `graph`、`undoManager`、`editor` 对象；
2. **单步 undo 提交**（AI 改动提交路径）：
   ```ts
   graph.model.execute(new contentWindow.ReplaceDiagram(graph.model.root, newModel))
   ```
   对齐上游 `commitDiagram`（editor-bridge.ts:473-509）——整个 AI 改动 = **一次 undo 操作**（用户 Ctrl+Z 一步撤销全部 AI 改动，FR-E4）；
3. **流式预览**：AI 生成过程中用 `preview` 通道（不进 undoManager，editor-bridge.ts:213-215,424）逐步更新画布，最终一次 `commitDiagram` 落 undo；
4. **选区读取** `readSelectionDetails`（:835）：返回选中 cell 的 id/label/style/geometry → 供「改选中」（FR-E6）；
5. **变更高亮** `highlightCells`（:738）：按 touched_ids 闪烁/描边变更形状（FR-A8）；
6. **降级**：若上游升级破坏内部 API（hook 失败）→ 自动降级为纯 postMessage 通道（load 即整页替换，AI 改动不可单步 undo），并在 UI 提示「桥接降级」。**这是 ADR-1 的显式回退路径**，由 06 的 L3 回归项监控。

## 6. 导出管线（FR-X1，P0）

```
常规 SVG/PNG/XML：
  前端 bridge.export(format) → iframe 内部渲染（draw.io 原生，EditorUi.exportSvg/exportImage，07.1 §4）
  → "export" 消息回传 dataURI → 前端/Rust 落盘
PDF（v0.1，02 ADR-9）：
  bridge.export("svg") → visio::converter（foreignObject→<text>，resvg 不支持 foreignObject）
  → resvg 光栅化 → printpdf 分页 → 写盘（纯 Rust、headless、CI 可验证）
  Windows v1.x 选项：WebView2 TryPrintToPdf 矢量高保真（无需 converter）
```

- 导出参数（scale/border/background）透传 draw.io；
- PNG 同时供 VLM 校验复用（03.1 §5，commit 后 ~100ms 取图，发送前降采样）；
- **Visio 管线**（FR-X2）= `export svg → visio::converter → 落盘`，见 03.3。

## 7. 画布上下文快照（FR-E5，供 AI）

- 每次用户发起 AI 消息前，前端 `bridge` 经 `save` 通道（临时 autosave 触发）或同源桥接直接读 `graph.model` 取**当前页 XML** + 页列表 + 选区；
- 发 `ai_chat_send { message, canvas: { page_xml, page_count, page_name, selection } }`；
- Rust 侧按 03.1 §3 的「差异才附」协议决定随消息附带的 XML 范围（控制 token）。

## 8. 状态与生命周期

| 事件 | 前端 store 动作 |
|------|----------------|
| iframe `init` | canvas_state = Ready，启用 load/导出按钮 |
| `save`（autosave） | 更新 store.diagram_xml_dirty，供 AI 上下文与「未保存提示」 |
| `load` 完成（drawio 无显式 ack → 以无 `error` 且定时快照对比成功判定） | 标记 applied，触发 100ms 后 PNG 快照（AI 轮次） |
| 桥接 hook 成功/失败 | bridge_state = Direct / MessageOnly（UI 角标） |

## 9. 风险与对策

| 风险 | 对策 |
|------|------|
| 上游 drawio 嵌入协议/内部 API 变更 | L3 同步回归项：每次 L1 换版跑 `tests/embed/` 协议集成测试（05 Step 2 建立） |
| 双 webview 内存（app.min.js 10MB + 主 UI） | 跟踪 NFR-4；后续可评估「仅嵌入 viewer 静态包 + 自绘工具条」瘦身（v1.x 优化项） |
| webview 引擎差异（WKWebView/WebView2/WebKitGTK）导出行为不一致 | 导出集成测试三平台 CI（05 Step 10 打包前矩阵） |
