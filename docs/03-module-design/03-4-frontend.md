# 03.4 · 前端框架设计（Svelte 5）

> **状态**：草案 ｜ **最后更新**：2026-10-29 ｜ **上游依据**：07.2 §8（React UI 不移植，交互设计参考 next-ai-draw-io）

---

## 1. 模块职责

drawio-rust 的宿主 UI：布局壳、聊天面板（AI 对话）、画布容器（iframe 桥接）、设置、同步面板、版本历史。对应 FR-A 系列 UI、FR-S1~S4。**不直连 LLM、不读 API Key、不做 XML 校验**（均在 Rust，见 02 ADR-2）。

## 2. 技术基线

- **Svelte 5**（runes：`$state` / `$derived` / `$effect`）+ TypeScript + **Vite**（sveltekit 不需要——单页桌面应用，用 vanilla svelte 模板 + 轻量路由）；
- 样式：Tailwind CSS 4（与 next-ai-draw-io 视觉语言可参考，组件自绘，不用 Radix）；
- 图标：lucide-svelte；
- 测试：vitest + @testing-library/svelte；E2E（P4）：playwright（驱动 Tauri dev）。

## 3. 应用结构

```
App.svelte                      # 布局：[左侧 ChatPanel | 中央 Canvas | 右侧（可折叠）Inspector/版本]
├─ lib/
│  ├─ canvas/
│  │  ├─ DrawioFrame.svelte     # <iframe> 容器 + bridge 生命周期（等 init、重连、降级角标）
│  │  ├─ bridge.ts              # postMessage 封装（03.2 §4 全量类型）+ 同源桥接客户端
│  │  └─ canvasStore.ts         # canvas_state / bridge_state / dirty / page_info（runes store）
│  ├─ chat/
│  │  ├─ ChatPanel.svelte       # 消息列表 + 输入框 + 模型选择 + 文件上传（P2 阶段）
│  │  ├─ Message.svelte         # 消息渲染：文本 / thinking / 工具调用卡（display/edit/append/library）
│  │  ├─ VersionCard.svelte     # 版本卡：缩略图 + diff 计数 + 对比/恢复/撤销按钮
│  │  └─ aiClient.ts            # invoke 封装 + Tauri 事件订阅（ai_stream/ai_tool_result/sync_*）→ store
│  ├─ settings/
│  │  └─ SettingsDialog.svelte  # Provider/模型/API Key（invoke 写 keyring）/校验模型/语言/主题
│  ├─ sync/
│  │  └─ SyncPanel.svelte       # 三个上游状态（pinned→latest 落后 N 提交）+ 立即同步 + 报告视图
│  ├─ inspector/
│  │  └─ Inspector.svelte       # 选中形状详情（供「改选中」入口，FR-E6）
│  └─ stores/
│     ├─ session.ts             # 当前会话：messages / versions / currentVersion
│     └─ appState.ts            # 布局/主题/语言/未保存标记
```

## 4. 关键交互流（前端视角）

### 4.1 AI 对话
```
输入发送 → aiClient.chatSend({message, canvas: await bridge.snapshot()})
事件 ai_stream{token}        → 当前助手消息追加（打字机，150ms 节流，对齐上游 experimental_throttle）
事件 ai_stream{tool_call}    → 工具调用卡（展开显示 xml 摘要/operations 表）
事件 ai_tool_result{...}     → bridge.load(xml)（或 bridge.commitDirect 单步 undo）
                               → 100ms 后 bridge.exportPng() 回传 Rust（VLM）
                               → VersionCard 追加（touched_ids 高亮由 bridge.highlight 执行）
事件 ai_turn_done            → 版本记录完成，输入框解锁
事件 ai_turn_error           → 错误消息（致命错误）/ 自动重试指示（非致命）
```

### 4.2 文件
```
打开：invoke("file_dialog", {kind:"open"}) → path → invoke("file_open", {path}) → {xml, meta}
      → bridge.load(xml) → 记录 currentFile
保存：bridge 取当前 xml（autosave 通道）→ invoke("file_save", {path, xml})
      （路径未定时先 file_dialog）
导出：invoke("export_file", {format, path?}) → 完成 toast（含 visio 转换计数）
```

### 4.3 设置
Provider 表单 → invoke("settings_save", {provider, model, apiKey?, baseUrl?})（Rust 校验 + 写 keyring）；
「测试连接」→ invoke("model_test", {...})（Rust 发最小请求，对齐上游 test-model-connection.ts 语义）。

### 4.4 同步
SyncPanel 每 5 分钟（或手动）invoke("sync_status") → 展示各上游 pinned/latest/落后提交数 + L2 资产 hash 漂移；
「立即同步」→ invoke("sync_run") → 事件 sync_progress/sync_done{report} → 报告视图（分级列出应用/报告/跳过项）。

## 5. 状态管理

- 全部 runes store（无第三方状态库）；跨组件共享用模块级 store（session/appState）；
- **单一事实源**：画布 XML 以 iframe 内 draw.io 为准（前端只持「dirty 标记 + 最近快照」，避免双源不一致）；AI 上下文每次发送前实时采集（03.2 §7）；
- 版本历史数据在 Rust（快照落盘），前端仅缓存展示用摘要 + 缩略图（PNG dataURI 由 Rust 事件携带）。

## 6. 与上游 UI 的继承关系（明确边界）

| 上游（React） | drawio-rust（Svelte 5） |
|---------------|----------------------|
| chat-engine.tsx（2000+ 行状态机） | **不迁移代码**；交互语义（自动重试、工具卡、thinking 展示）在 aiClient + store 重实现，状态机本体在 Rust（03.1 §6） |
| drawio-frame.tsx | DrawioFrame.svelte + bridge.ts（协议对齐 03.2 §4） |
| model-config-dialog / model-selector | SettingsDialog（Provider 数据表来自 Rust invoke） |
| 版本卡 / 对比 / 恢复 | VersionCard（数据来自 Rust 版本事件） |
| 4 语言 i18n | MVP 中/英（FR-S3），i18n 用 svelte-i18n，词条结构参考上游 docs 语言包 |
| 深色模式 | 主题变量 + `configure{theme}` 同步 drawio（03.2 §4） |

## 7. 性能与质量要求

- 画布区域零 re-render 干扰：ChatPanel 高频流式更新不得触发 Canvas 重渲染（组件隔离 + 150ms 节流）；
- 首屏（应用启动到画布可用）< 5s（本地）；
- ESLint 等价物（svelte-check + tsc）CI 必过；
- 可访问性：键盘可完成全部操作（对话、导出、版本恢复）。
