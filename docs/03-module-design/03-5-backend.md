# 03.5 · 后端服务设计（Rust / Tauri 2 主进程）

> **状态**：草案 ｜ **最后更新**：2026-10-29 ｜ **上游依据**：07.2 §8 分类 A（纯逻辑移植清单）

---

## 1. 模块职责

Tauri 2 主进程（Rust）承载：AI 编排、文档服务（文件/页/diff）、Visio 转换、导出编排、配置与密钥、上游同步。对前端暴露 **Tauri commands**（invoke）与 **events**（推送），清单见 04。

## 2. Crate 结构

```toml
# Cargo.toml（workspace）
[workspace]
members = ["src-tauri"]
```

| crate | 内容 | 依赖边界 |
|-------|------|----------|
| `src-tauri` | **单一后端 crate**：Tauri 入口、命令注册、事件、keyring（含 Linux 0600 文件降级）、导出编排，及纯逻辑模块 `ai/`（prepare/xmlfix/compact/edit/tools/vlm/prompt/providers）、`doc/`（fileio/diff）、`visio/`、`session/`（版本快照）、`sync/`（manifest/fetch/apply/report）与 `bin/drawio-sync.rs`（同步 CLI） | quick-xml、regex、reqwest、tokio、schemars、zip、flate2、resvg、printpdf、image、glob；shelling out 到系统 git（06 §5，不引 git2） |

**设计意图**：单一后端 crate 消除跨 crate 依赖边界；纯逻辑模块（`ai/doc/visio/session`）无 UI/Tauri 依赖，可纯单测 + 上游用例对齐（ADR-2）；`drawio-sync` 作为 bin 目标可在 CI/开发机运行（06）。

## 3. 各服务要点（详细设计见对应 03.x 分册）

| 服务 | 分册 | 要点 |
|------|------|------|
| AI 编排 | 03.1 | 工具循环状态机；provider 数据表 + 3 协议客户端；VLM 校验；重试上限（结构校验 N 次、VLM 3 次） |
| 文档服务 | 03.2/§7 | `.drawio` 双格式读写：明文 XML 与压缩格式（**zip 容器 + raw-deflate 条目**：`PK` 头检测 → zip crate 解析 → flate2 RawDecoder；写默认明文，可选压缩开关，对齐 draw.io 兼容）；页提取/替换/放置（port diagram-pages.ts:89/109/141，保留其他页与 file vars）；cell 级 diff（port diagram-diff.ts:159-191，signature=规范化 style+geometry） |
| Visio | 03.3 | 纯函数转换器 + 管线编排 |
| 导出 | 03.2/§6 | 经 iframe 取 SVG/PNG/XML；PDF 主路径 = resvg+printpdf（复用 visio converter，ADR-9）；visio 管线调用 |
| 会话/版本 | 03.1/§8 | `sessions/<id>/`：meta.json + versions/（快照+缩略图），上限 20 |
| 配置 | §4 | 下详 |
| 同步 | 06 | 下详 |

## 4. 配置与密钥（`settings/store.rs`）

- 配置持久化：`%APPDATA%/drawio-rust/config.toml`（provider 选择、模型、语言、主题、导出默认参数、同步开关）——**不含密钥**；
- API Key：`keyring` crate → OS 钥匙串（Windows Credential Manager / macOS Keychain / Linux Secret Service），条目名 `drawio-rust:<provider_id>`（NFR-1）；
- **keyring 可用性与降级**（Linux headless/裸桌面/CI 常无 Secret Service）：启动探测；不可用 → **显式**降级 `%APPDATA%/drawio-rust/keys.toml`（0600 权限）+ 持久 UI 警示条 + 设置对话框明示；绝不静默降级；
- 读取路径：AI 循环开始时取 key（keyring 或降级文件）构建 provider client；**key 永不进入日志/事件/前端**；
- 自定义端点 SSRF 防护：解析 URL，拒绝 localhost/私网段（10/8、172.16/12、192.168/16、169.254/16、::1、fc00::/7）与云元数据地址（port `lib/ssrf-protection.ts` 规则）；**显式例外**：Ollama 默认 `http://localhost:11434`（本地推理，FR-A9）及用户显式允许的其他 localhost 端口（警示提示）。

## 5. 错误与日志

- `thiserror` 定义域错误：`AiError`（provider/流式/校验/工具）、`DocError`、`VisioError`、`ExportError`、`SyncError`、`SettingsError`；对外统一 `AppError`（含 code 字符串供前端分类处理）；
- `tracing` + 按文件滚动（应用数据目录 logs/），NFR-7；AI 请求/响应体**脱敏**（key 头、user 内容截断）；
- 事件错误载荷：`{ code, message, retryable }`。

## 6. Tauri 集成要点

- 单主窗口；`tauri.conf.json` 安全策略：CSP 允许 `tauri:` 与本地资源；`assetProtocol` 启用，**仅**伺服会话缩略图等小资源（webapp 走前端 public/ 同源伺服，见 ADR-1）；
- 命令全部 async（tokio），耗时操作（同步、导出）走后台任务 + 进度事件，不阻塞 UI；
- 崩溃恢复：会话目录 + 每消息 checkpoint（messages.jsonl 追加写），重启可恢复会话。

## 7. 可测试性矩阵

| 层 | 手段 | 覆盖要求 |
|----|------|----------|
| drawio-core 纯逻辑 | cargo test（fixtures 对齐上游用例） | 行覆盖 ≥80%（ci 门禁） |
| provider 客户端 | wiremock 集成测试（OpenAI/Anthropic/Google 各一组流式+错误用例） | 每协议 ≥4 用例 |
| Tauri 命令 | 命令层薄测试（参数校验+错误映射）；重逻辑已在 core | 每命令 ≥1 |
| 同步 | 临时 git 仓库 fixture（创建 commit 序列 → 断言分级行为） | L1/L2/L3 各 ≥2 |
| 端到端 | P4 起 playwright 驱动 dev（打开→生成→导出→visio 文件存在） | 3 条主路径 |
