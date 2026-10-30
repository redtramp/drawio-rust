# 05 · 实施计划（分步蓝图）

> **状态**：已定稿 ｜ **最后更新**：2026-10-29
> **约定**：每个 Step 自包含（Context Brief 足够冷启动执行）；依赖用 `←` 标注；`‖` 表示可并行。验证命令（V）与退出标准（X）为硬性门槛（人工验收项显式标注「可延迟、不阻塞合并」，见 S9）。git/PR 流程：主干 `main`，每 Step 一个分支/PR（`step/N-<slug>`），CI 全绿方可合入。

---

## 依赖总览

```
S1 脚手架 ─┬─ S2 嵌入画布 ─ S3 文件读写 ─ S4 常规导出 ─┐
           │                                          ├─ S9 Visio 导出
           └─ S7 provider 客户端 ─ S8 AI 准备/校验 ─ S5 生成 ─ S6 编辑/版本/VLM
S10 同步工具（仅依赖 S1 仓库结构，可与 S2-S6 并行）
S11 设置/i18n/主题/打包（收尾）
```

**并行组**：
- 组 A（编辑器线）：S2 → S3 → S4
- 组 B（AI 线）：S7 → S8 → S5 → S6（S7 与 S2/S3 并行）
- 组 C（同步线）：S10（与 A/B 全并行，只依赖 S1）
- S9 仅依赖 S4（Visio 转换器已在 S4 随 PDF 实现；L3 同步报告不阻塞实施）；S11 收尾。

**模型分层**：S1/S2/S4/S9 用默认模型；S8（500 行逻辑平移+用例对齐）、S6（状态机）、S10（git 流程）用最强模型。

---

## S1 · 项目脚手架

- **Context**：新建 `drawio-rust`（已存在空目录）。目标：Tauri 2 + Svelte 5 + TS + Vite + Tailwind 4 可跑通的最小壳，含 workspace 三 crate（src-tauri / drawio-core / drawio-sync 空壳）。
- **任务**：
  1. `pnpm create tauri-app`（svelte-ts 模板）生成前端；`cargo new` 生成 workspace 两 crate（`drawio-core` 空 lib、`drawio-sync` 空 bin，均带 CI 可构建）；
  2. 前端：Tailwind 4、lucide-svelte、目录骨架（`lib/canvas|chat|settings|sync|stores`，见 03.4 §3）、布局壳（左 Chat 占位 / 中 Canvas 占位 / 右侧栏占位）；
  3. 后端：`app_info` 命令 + 事件骨架、`tracing` 日志初始化、`AppError` 错误类型；
  4. CI（`.github/workflows/ci.yml`）：`pnpm build && cargo test && cargo clippy -D warnings`（三平台 matrix: ubuntu/macos/windows）。
- **验证**：`pnpm tauri dev` 开窗；`cargo test` 过；CI 绿。
- **退出标准**：Linux（开发机）dev 可启动 + 三平台 CI 构建/测试绿（mac/win 的 dev 启动由开发机人工验证并记录，不作 CI 门禁）；CI 模板就绪；目录与 02 §2.3 一致。
- **回滚**：整 Step 为新增文件，revert 即可。

## S2 · 嵌入 draw.io 画布（postMessage 通道）

- **Context**：S1 完成后。目标：iframe 嵌入 draw.io webapp，能打开空画布并手工编辑。依赖 03.2 §2-4、04 §3。
- **任务**：
  1. **首次 vendor**：把 `sample/drawio/src/main/webapp/*` 拷入 `public/drawio/`（保留其 LICENSE/NOTICE——NFR-3），记录 pinned commit `9034053fb` 到 `upstream.toml`（初始文件，04 §5.1）；`public/drawio/` **直接入库**（ADR-3，自包含优先）；创建 `sync/vendor-keep/patches/`（初始空清单）；
  2. Vite：`public/` 自动伺服（dev `http://localhost:<port>/drawio/`、prod `tauri://localhost/drawio/`，与主页面同源，ADR-1/ADR-3）；asset protocol 仅配置会话缩略图等小资源；
  3. `DrawioFrame.svelte` + `bridge.ts`：iframe（URL 参数 03.2 §3）、origin 白名单、`init` 等待、`load/save/export/error/message` 全量类型（04 §3）；canvasStore 状态机；
  4. **协议校准**：dev 环境实际跑通 load（手造一段最小 mxfile）/export(svg,png,xml)/autosave，把实际行为冻结进 `tests/fixtures/embed/`；
  5. 集成测试 `tests/embed/`（headless：启动 dev，注入 fixture XML，断言 export 回传可解析）——**这是此后每次 S10 L1 换版的回归门禁**；
  6. **prod 同源验证**：`tauri build` 后跑打包应用，断言 iframe origin == 主页面 origin（`tauri://localhost`）且在白名单内、postMessage load/export 往返成功（人工 checklist 项，记录进退出记录，防 dev/prod 分叉，ADR-1）。
- **验证**：`pnpm tauri dev` → 画布可编辑；`cargo test -p drawio-core --test embed` 过；CI 绿。
- **退出标准**：空白画布可增删形状；load/export 三类格式回传正确；embed 回归测试建立；**prod 包 iframe 同源 + postMessage 往返验证通过**（任务 6）。
- **回滚**：revert Step；vendor 目录独立目录可单独回退。

## S3 · 文件打开/保存

- **Context**：S2 后。目标：`.drawio` 打开/保存（明文+deflate 双格式）。依赖 03.5 §3、04 §1。
- **任务**：
  1. `drawio-core::doc::fileio`：读（明文直通；压缩 = zip 容器 + raw-deflate 条目：PK 头检测 → zip 解析 → flate2 RawDecoder；坏数据 `DocError`）、写（默认明文 + 可选压缩开关）；
  2. 命令 `file_dialog/file_open/file_save`（含 tauri-plugin-dialog/fs 权限配置）；
  3. 前端：打开/保存按钮 + 未保存标记（autosave `save` 事件驱动 dirty）+ 当前文件路径栏；
  4. 测试：roundtrip（明文/压缩各 ≥3 个真实样本，样本从 sample 仓库与手造）；`is_same_document` 级别断言。
- **验证**：打开 sample 中真实 `.drawio`（从 drawio 仓库/网络样例收 ≥3 个 fixture）→ 画布正确渲染 → 另存 → 用官方 draw.io 可打开（人工抽验 1 次）。
- **退出标准**：双格式读写 roundtrip 全过；与官方 draw.io 互开不损坏。
- **回滚**：revert；纯增量。

## S4 · 常规导出（png/svg/xml/pdf）

- **Context**：S3 后。目标：一键导出 png/svg/xml/pdf。依赖 03.2 §6、04 §1。
- **任务**：
  1. 前端导出对话框（格式/scale/border/background）；
  2. **Visio 转换器实现（自 S9 上移，PDF 依赖它）**：`src-tauri/src/visio/converter.rs` 按 03.3 §3 逐段实现（quick-xml）+ 03.3 §6 全组测试（含 **Python 原版 vs Rust 输出对齐**：同 fixture 双跑，text 节点属性级 diff）；
  3. `export_file` 命令编排：png/svg/xml 走 bridge.export 回传 dataURI → 写盘；**pdf 主路径（ADR-9）**：bridge.export("svg") → converter → resvg → printpdf → 写盘（进度事件）；
  4. `export_progress` 事件 + 前端进度条；
  5. 集成测试：svg/png 数据非空且 magic 正确；pdf（resvg 路径）Linux CI 解析验证（有效 PDF + 页数正确），headless 无显示依赖。
- **验证**：导出 4 格式产物可解析（svg/png magic、pdf 头 + 页数）；CI 绿（headless）。
- **退出标准**：FR-X1 达成；导出耗时 < 5s（10 形状页面）；pdf 为光栅格式 → UI 明示「矢量高保真请导出 SVG」（ADR-9）。
- **回滚**：revert；pdf 子项可单独摘除不阻塞 png/svg。

## S5 · AI 对话生成（display/append/get_library）

- **Context**：S7+S8 后（S2 必须已完成，画布可用）。目标：一句话 → 图形上画布。依赖 03.1 §2-3/§5(仅结构层)/§6-7。
- **任务**：
  1. L2 资产首次抽取入库：`system-prompt.md`（从 `lib/system-prompts.ts` 的 DEFAULT/STYLE 常量抽出为 md，保持文本逐字一致）、`shape-libraries/*.md`、`model-catalog.json` → `src-tauri/assets/prompt/`（来源文件头注释）；
  2. `ai/prompt.rs` 组装（含 canvas context 差异判定，接 `doc/diff` 的简化版：整页 hash 比对即可）；
  3. `ai/orchestrator.rs`：流式 → 事件（token/thinking/tool_call）；`display_diagram`/`append_diagram` → S8 的 prepare 管线 → `ai_tool_result`；`get_shape_library` 读本地 md 回喂；
  4. 前端 ChatPanel MVP：输入框、消息流、工具卡、`ai_tool_result` → bridge.load、模型/Provider 选择（数据来自 `providers_list`）；
  5. 错误路径：provider 401/429/超时 → `ai_turn_error`（UI 提示重试）。
- **验证**：`cargo test`（prompt 组装/工具解析 ≥10 用例）；手动：openai 兼容 + anthropic 各跑通 3 个提示词（AWS 架构/ER 图/流程），图形正确上画布；截断场景（小 max_tokens）触发 append 续写成功 1 次。
- **退出标准**：FR-A1/A3/A9（MVP 集）达成；首 token < 3s（本地实测）。
- **回滚**：AI 功能独立开关（settings `ai.enabled`），关闭即回到纯编辑器。

## S6 · AI 编辑 / 版本 / VLM 校验 / 流式高亮

- **Context**：S5 后。目标：改选中、单步 undo、版本卡、VLM 重试。依赖 03.1 §4-5/§8、03.2 §5（同源桥接）。**最强模型**。
- **任务**：
  1. `ai/edit.rs`：port `edit-diagram.ts`（update/add/delete + 级联），单测对齐上游 fixture；
  2. 同源桥接：`watchForEditorUi` 挂接 + `commitDiagram`（ReplaceDiagram 单步 undo）+ `readSelectionDetails` + `highlightCells` + **降级路径**（hook 失败 → MessageOnly，UI 角标）；
  3. 流式预览：display 过程中 preview 通道逐步渲染（不进 undo），完成一次性 commit；
  4. 版本：`ai/versions.rs` 快照（上限 20）+ `version_new` 事件 + VersionCard（对比=diff 计数+touched 高亮、恢复=load 快照、撤销=Ctrl+Z 联动）；
  5. VLM：`ai/vlm.rs`（PNG 采集 → 校验模型调用 → 结果反馈，≤3 次、末次放行、故障放行）；「改选中」入口（Inspector 选中 → 对话上下文注入 selection）。
- **验证**：对齐上游用例（edit 操作 10 组）；手动：选中 2 形状 → 「改成红色」→ 仅 2 形状变化且高亮 → Ctrl+Z 一步全撤；VLM 故意诱导错误（提示词注入坏 XML）→ 自动重试 ≤3 收敛或放行；桥接降级演练（临时改 hook 选择器 → 降级不崩溃）。
- **退出标准**：FR-E4/E6、FR-A2/A5/A6/A8 达成。
- **回滚**：版本/ VLM 独立开关；桥接降级是内建路径。

## S7 · Provider 客户端层

- **Context**：与 S2/S3 并行（只依赖 S1）。目标：reqwest 直连的 provider 抽象。依赖 03.1 §7、02 ADR-7。
- **任务**：
  1. `providers.toml` 数据表（MVP 10+：openai/deepseek/qwen/doubao/glm/kimi/minimax/openrouter/ollama/sglang + anthropic + google + custom）；
  2. 三协议客户端：`openai_compat`（chat completions + SSE 流式 + tools 参数）、`anthropic`（messages + thinking）、`google`（generateContent）；
  3. `model_test` 命令 + SSRF 防护（含 Ollama localhost 显式例外）+ keyring 读写（含 Linux 无 Secret Service 检测 → 0600 文件降级 + UI 警示）+ `settings_*` 命令 + SettingsDialog（前端）；
  4. wiremock 测试：每协议 流式/错误(401/429/超时)/工具调用 各用例。
- **验证**：`cargo test` wiremock 全过；手动对 3 家真实端点 `model_test` 通过；无 Secret Service 环境验证降级文件 0600 + UI 警示（或单测 mock keyring 缺失）。
- **退出标准**：NFR-1 达成（key 不落盘明文，审计配置目录）；S5 可直接依赖本层接口。
- **回滚**：独立 crate 内模块，revert 即可。

## S8 · AI 准备/校验管线（纯逻辑平移）

- **Context**：S7 后（与 S2 线并行可行，但建议 S7 后）。目标：prepare/xmlfix/compact/edit 预研/doc-diff/pages 的 Rust 平移 + **上游用例对齐**。**最强模型**。依赖 03.1 §4、03.5 §3、04 §6。
- **任务**：
  1. 移植清单（03.1 §9 表 A 级）逐文件平移：`prepare.rs`/`xmlfix.rs`（autoFix ~500 行）/`compact.rs`/`style-classes` 合并入 prepare；`doc/pages.rs`（含 deflate 页提取）、`doc/diff.rs`（signature 规范化）；
  2. **用例对齐基建（基线生成）**：`tests/fixtures/alignment/` 收 20 组 XML（有效/缺引号/重复 id/嵌套 cell/orphan point/截断/样式类/多页…）；**前置盘点上游测试现状**（packages/mcp-server、lib 目录）：有可跑测试 → 直接跑生成期望基线；无 → 搭临时 harness（tsx/esbuild 加载上游源函数）对 20 组 fixture 跑上游逻辑生成期望基线；仍不可行 → 按代码语义手工构造基线（双人复核）→ 基线入库（`tests/fixtures/alignment/baseline/`），Rust 断言逐组一致；
  3. 差异处理：上游实现有 bug 且确认无副作用 → 在 docs 记录偏差并**保持对齐**（行为一致优先，修 bug 单独立项）；
  4. doc 行覆盖 ≥80% 门禁（cargo-llvm-cov，CI 加）。
- **验证**：`cargo test -p drawio-core` 全过（含 20 组对齐）；CI 覆盖率门禁绿。
- **退出标准**：对齐 20/20；S5/S6 可零修改调用。
- **回滚**：独立模块，revert。

## S9 · Visio 兼容导出（格式与验收）

- **Context**：S4 后（Visio 转换器及其全组测试已在 S4 实现）。目标：FR-X2（visio-svg 导出格式 + 人工验收）。依赖 03.3。
- **任务**：
  1. `export_file{format:"visio-svg"}` 格式 + UI（转换计数 + Visio 导入引导文案，03.3 §4）；
  2. 真实样本：draw.io 导 3 类图（流程图/架构图/含中文标签）→ 转换 → **人工 Visio 验收**（有 Visio 环境时执行；无环境则记录「待验收」，**不阻塞合并**）；
  3. Python 原版 vs Rust 输出对齐 diff 全量复跑（S4 转换器测试继承项）通过；
  4. 已知限制 UI 提示 + PNG 底图法引导（v1.x 增强项，Q3）。
- **验证**：`cargo test -p drawio-core --test visio` 全过；对齐 diff 为空（或仅序列化格式差异白名单）。
- **退出标准**：**自动（硬）**——03.3 §6 用例全过 + 对齐 diff 干净；**人工（可延迟、不阻塞）**——至少 1 个真实 SVG 在 Visio 中文字位置正确（无环境时挂「待验收」）。
- **回滚**：格式选项摘除即可，纯增量。

## S10 · 上游同步工具（drawio-sync）

- **Context**：S1 后全并行。目标：06 全套。依赖 04 §5。**最强模型**（git 语义）。
- **任务**：
  1. `src-tauri`：manifest 解析（04 §5.1 schema + 校验）、fetch（bare mirror 缓存在 `.sync/mirrors/<id>`，`git fetch`）、diff `pinned..HEAD`；
  2. 应用器：L1（整目录 rsync 式换版 + 本地有意修改补丁重放 `sync/vendor-keep/patches/`）、L2（asset 抽取规则：TS 常量→md 的确定性抽取器 + hash 比对覆盖）、L3（按 logic 映射表生成 diff 摘要报告）；
  3. 换版后**自动跑回归**：S2 的 `tests/embed/` + S8 对齐测试 + S9 visio 对齐测试 → 结果入报告；
  4. 报告：`sync/reports/<ts>.md`（04 §5.2 schema 渲染）+ `SyncStatus/SyncReport` Tauri 命令/事件 + SyncPanel（前端）；
  5. 测试：临时 git 仓库 fixture（03.5 §7）L1/L2/L3 行为各 ≥2；
  6. CI 增加 `sync-check` job（每周/手动 workflow_dispatch：fetch 上游，报告 L2 漂移与 L3 变更，不自动改主干）。
- **验证**：`cargo test -p drawio-sync` 全过；真实跑一次 `drawio-sync run --all`（当前 pinned=HEAD，应报告 0 变更）；人为把 pinned 回退 3 个 commit 跑一次，验证 L2 覆盖+报告正确。
- **退出标准**：FR-S2 达成；NFR-5：一次命令完成三上游同步 + 报告 + 回归。
- **回滚**：同步操作前对 `vendor/` 与 `assets/prompt/` 打 git tag `pre-sync-<ts>`；失败 `git reset` 回 tag。

## S11 · 设置完善 / i18n / 主题 / 打包签名

- **Context**：S5-S10 后收尾。目标：v0.5 可发布。
- **任务**：
  1. 中/英 i18n（svelte-i18n，词条结构参考上游语言包）、深色模式（含 `configure{theme}` 同步 drawio）；
  2. 首次启动引导（Provider 配置向导）；会话恢复（messages.jsonl）；
  3. 打包：tauri-bundler 三平台产物（msi/dmg/AppImage+deb）、图标、版本号、changelog；（签名证书视环境，无则文档说明）；
  4. 文档收尾：README（用户向）、CHANGELOG、`docs` 状态改「已定稿」；
  5. 许可合规检查：安装包含 draw.io LICENSE/NOTICE（NFR-3 审计清单走一遍）。
- **验证**：三平台安装包安装/启动/主路径（打开→AI 生成→导出 visio）冒烟；i18n 双语言切换无硬编码残留（grep 门禁）。
- **退出标准**：v0.5 打 tag；1.8 表中 P4 全项达成。
- **回滚**：发布物不可回滚 → 仅前移修复；开发态 revert。

---

## 里程碑映射（对齐 01 §1.8）

| 版本 | 完成 Steps | 验收 |
|------|-----------|------|
| v0.1（MVP） | S1-S4 | P0 全项 |
| v0.2 | +S5(+S7/S8) | P1 生成 |
| v0.3 | +S6 | P2 编辑/版本/VLM |
| v0.4 | +S9 | P3 Visio |
| v0.5 | +S10(+S11 部分) | P4 同步 |
| v0.5 收尾 | S11 | 发布 |

## 风险登记（Top）

| 风险 | 影响 | 缓解 |
|------|------|------|
| 同源桥接被上游升级破坏 | 单步 undo/高亮失效 | 内建降级（03.2 §5）+ S10 L1 换版回归门禁（S2 建立） |
| autoFix 平移行为偏差 | AI 产出校验不一致 | S8 用例对齐 20/20 硬门槛 + 偏差记录 |
| vendor 目录膨胀（~20MB+） | 仓库/克隆慢 | 接受（自包含优先，ADR-3）；v1.x 可切 asset 仓库/子模块 |
| PDF 主路径为光栅（文字不可选取） | 交付质量预期差 | UI 引导以 SVG 为主；Windows WebView2 高保真路径 v1.x（ADR-9） |
| 三平台 webview 导出差异 | pdf/png 不一致 | S4 三平台冒烟 + 问题单 |
| Visio 人工验收无环境 | P3 退出标准卡住 | 标记「待验收」不阻塞合并，CI 保留对齐测试 |

---

## 附录 A · 测试策略（与环境约束）

| 层 | 载体 | 环境约束 | CI 策略 |
|----|------|----------|---------|
| 单元（drawio-core 纯逻辑） | cargo test + cargo-llvm-cov（≥80% 门禁） | 无外部依赖 | 每 PR |
| 上游对齐（S8 基线 / S9 Python 双跑） | fixtures + baseline 目录 | Python 双跑需 python3（ubuntu runner 有；缺失时跳过并标记，合并前必须过一次） | 每 PR（尽力） |
| Provider 客户端 | wiremock | **CI 无真实 API key**——AI 路径全部走 mock provider 覆盖 | 每 PR |
| Embed 协议集成（tests/embed/） | dev + 浏览器驱动 | 需显示 → ubuntu runner 用 xvfb + webkit2gtk 系统依赖（独立 job） | nightly + S2/S10 换版时 |
| E2E 主路径（打开→AI 生成(mock)→导出→visio 文件存在） | playwright + tauri driver | 同上；AI 走 wiremock provider | nightly |
| prod 同源验证 | 打包应用 + checklist | 三平台 dev 机（mac/win 开发机、linux runner） | S2 门禁 + 每次发布前 |
| Visio 人工验收 | 人工 | CI 无 Visio → 受控环境执行（「待验收」机制） | — |

## 附录 B · 工作量估算（1 人月 = 20 人日）

| Step | 估算（人日） | 备注 |
|------|-----------|------|
| S1 脚手架 | 3 | 模板 + CI |
| S2 画布嵌入 | 5–7 | 含协议校准 + prod 同源验证 |
| S3 文件读写 | 3 | |
| S4 导出 + converter + PDF | 5 | converter 平移 + resvg/printpdf 集成 |
| S7 Provider 层 | 5 | 3 协议客户端 + wiremock |
| S8 纯逻辑平移 | 8–10 | autoFix ~500 行 + 20 组对齐（最强模型） |
| S5 AI 生成 | 6 | |
| S6 AI 编辑/版本/VLM | 8–10 | 同源桥接为主要不确定项（最强模型） |
| S9 Visio 格式 + 验收 | 3–4 | |
| S10 同步工具 | 6–8 | git 语义 + 三级应用器 + 测试（最强模型） |
| S11 收尾/打包 | 5–7 | i18n/主题/签名/打包 |
| **合计** | **≈57–68 人日** | 1 人 ≈ 3 个月；2 人（编辑器线/AI 线并行）≈ 1.5 个月到 v0.5 |

假设：开发者熟悉 Rust + Web 前端；上游协议无重大破坏；同源桥接（S6）若被上游内部 API 变更卡住，该步降级为纯 postMessage 模式（功能降级、进度不阻）。
