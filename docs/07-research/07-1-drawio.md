# 07.1 · 调研：drawio（jgraph/drawio）

> **状态**：已完成 ｜ **调研时点**：2026-10-29 ｜ **上游**：`github.com/jgraph/drawio` branch `dev` @ `9034053fb`（32.4.1 release）

---

## 1. 仓库布局

```
drawio/
├─ LICENSE / README.md / VERSION / ChangeLog
├─ .github/workflows/            # CI（war.yml 构建 WAR）
├─ etc/
│  ├─ build/                     # Ant 构建（build.xml + compiler.jar + build.properties）
│  ├─ dependencies/ i18n/ imageResize/ integrate/   # 构建工具链辅助
└─ src/main/
   ├─ server/                    # Java 后端（26 个 Servlet：云认证/代理/缓存）
   └─ webapp/                    # ★ 前端全部（index.html、js/、mxgraph/、shapes/、styles/、resources/、images/、WEB-INF/）
```

- `webapp/js/` 196 个 JS、`mxgraph/src/` 115 个、`shapes/` 48 个、Java 26 个、i18n 61 个 txt。**全为 JS，无 TS，无运行时构建**。

## 2. 编辑器内核

- **核心库**：`src/main/webapp/mxgraph/`
  - `mxClient.js`：已编译 minified 单文件 bundle（1873 行，VERSION 32.4.1，`mxgraph/mxClient.js:1`）；
  - `mxgraph/src/`：115 个可读源码文件（`src/view/mxGraph.js`、`src/util/mxImageExport.js` 等）。
- **应用层**：
  - `js/grapheditor/`：`Editor.js`、`EditorUi.js`、`Graph.js`、`Actions.js`、`Dialogs.js`；
  - `js/diagramly/`：`App.js`（主应用）、`EditorUi.js`（**~3.3 万行**）、`ElectronApp.js`。
- **入口链**：`index.html:25` 载 `js/bootstrap.js` → `:90` 载 `js/main.js` → `App.main()`；`bootstrap.js:324` 动态加载 `mxClient.js`，`:359` 加载 `js/app.min.js`（dev 模式逐文件加载 `:321-329`）。
- **构建产物**：`.min.js` 为**提交入库的 Closure 编译产物**（非 CI 生成必需）：`app.min.js` 10.1MB / `viewer-static.min.js` 4.4MB / `viewer.min.js` 2.9MB / `extensions.min.js` / `mxClient.js`。

## 3. 桌面版构建体系

- **本 checkout 不含 Electron 主进程**：仓库根无 `package.json`/`main.js`（仅 `etc/dependencies`、`etc/imageResize` 两个构建工具用）。上游 jgraph/drawio 的 Electron 主进程源码**不公开**，桌面版以二进制分发。
- 构建：`etc/build/build.xml`（Ant + 本地 `compiler.jar` Closure 编译器，`build.properties:1`），产出 7 个 bundle（`build.xml:20-27`）。
- 渲染进程经注入的 `electron` 对象与主进程通信（`js/diagramly/ElectronApp.js:28`）；UA 检测在 `bootstrap.js:71-72`。

## 4. 导出管线

- UI 入口：`grapheditor/Actions.js:134`（`export...` → `ExportDialog`，`Dialogs.js:3172` / `exportFile:3629`）；
- 核心方法：`js/diagramly/EditorUi.js`——`exportSvg:8692`、`exportImage:12501`、`exportToCanvas:33051`（底层 `mxgraph/src/util/mxImageExport.js`）；
- **无头导出页**：`export3.html:20-22`（静态载 app.min.js + extensions.min.js + export.js）+ `js/export.js`（`render(data):162`；桌面/CLI 取 SVG 的监听 `:1006-1012`）；
- 桌面 vs Web：Web 版页内渲染 canvas/SVG 后下载；桌面版被 `ElectronApp.js` 接管，以 `mxElectronRequest('export', …)` 发主进程（`ElectronApp.js:168` PDF 打印、`:2753`、`:2851`），主进程开隐藏窗口跑 `export3.html`（主进程代码不在仓库）。

## 5. 可嵌入性（对 drawio-rust 最关键）

- **独立 Web 构建**：`js/viewer.min.js`（5529 行，独立查看器/lightbox）与 `viewer-static.min.js`（8990 行，内嵌 shapes），零 Electron 依赖，可静态部署；
- **iframe 嵌入**：URL 参数 `?embed=1` / `embedInline`（`viewer.min.js:18,30`），宿主通信走 postMessage（如导出回传 SVG dataURI，`diagramly/EditorUi.js:26634-26640`）；`diagramly/Embed.js`、`EmbedFile.js` 提供嵌入编辑；服务端代理 `server/java/com/mxgraph/online/EmbedServlet2.java`；
- **页内组件**：`diagramly/GraphViewer.js:12`（`GraphViewer` 类）；
- **无头渲染**：`export3.html` / `export.js` 即 headless 管线（桌面/CLI 共用）；无 npm 包形式 SDK，嵌入依赖 URL 参数 + postMessage 协议；
- **离线可用**：编辑/导出纯前端（Java server 仅云服务/代理），静态伺服 webapp 即可完整工作（next-ai-draw-io 生产验证：下载 draw.war 解包后 iframe 嵌入）。

## 6. 规模

| 项 | 量级 |
|----|------|
| `webapp/js/` | 196 文件 |
| `mxgraph/src/` | 115 文件 |
| `shapes/` | 48 文件 |
| Java Servlet | 26 文件 |
| `app.min.js` | 10.1 MB（16998 行） |
| `viewer-static.min.js` / `viewer.min.js` / `extensions.min.js` / `mxClient.js` | 4.4 / 2.9 / ~1 / 1-2 MB |
| `diagramly/EditorUi.js`（单文件源） | ~33,000 行 |

## 7. 许可证

- `LICENSE:1-3`：**Apache License 2.0**（JGraph Holdings / draw.io AG）——**不是 GPLv3**；
- 要点：可自由修改/再分发（含商业）；条件：附许可证副本、修改文件加显著声明、保留原版权与 NOTICE（`LICENSE:89-128`）；**不授予商标权**，不得声称与 draw.io 关联（`LICENSE:138-141`）；
- 「src 与产物」：本仓库是公开 src 子集（前端 + Java + 构建脚本）；Electron 主进程源码不公开；
- 第三方子许可：`libavoid-js` 为 **LGPL-2.1**（`js/libavoid-js/LICENSE:1-2`）、DOMPurify Apache/MPL（`viewer.min.js:5`）、pako MIT（`viewer.min.js:12`），再分发需各自合规。

## 8. 对 drawio-rust 的结论

1. **整包 vendor `src/main/webapp/` 并 iframe 嵌入**是继承编辑能力的唯一现实路径（ADR-1）；min 产物已入库，无需构建；
2. 桌面壳必须自建（Tauri 2）——上游 Electron 主进程不可得；导出不依赖它：SVG/PNG/XML 走 iframe postMessage 原生管线，PDF 由 drawio-rust 以 Rust 实现（resvg 主路径 / Windows WebView2 高保真，02 ADR-9）；
3. 同步层级 = **L1 整目录换版**（webapp 是编译耦合体，文件级 merge 无意义）+ embed 协议回归门禁（06 §4.1）；
4. 合规义务：保留 LICENSE/NOTICE、修改声明、商标边界（NFR-3，S11 审计清单）。
