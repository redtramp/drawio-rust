# 06 · 上游同步机制

> **状态**：草案 ｜ **最后更新**：2026-10-29 ｜ **依据**：02 ADR-3/ADR-6、04 §5、05 S10

---

## 1. 目标与原则

**目标**：三个上游（drawio / next-ai-draw-io / drawio-to-visio）发布更新后，一条命令把 drawio-rust 同步到最新，并给出可审计的报告与回归结果。

**原则**：
1. **drawio-rust 是再架构不是拷贝** → 同步必须分级：能整拷的整拷（L1），能文本对齐的文本对齐（L2），已翻译的只报告（L3）。**永不自动合并 L3 代码**。
2. **可审计**：每次同步产生 git tag（`pre-sync-<ts>` / `sync-<ts>`）+ 报告文件 + 上游 commit 指针更新（`upstream.toml`）。
3. **可回滚**：L1/L2 应用前打 tag；L3 只读。
4. **回归门禁**：L1 换版（drawio 内核）必须过 embed 协议回归 + 对齐测试，失败则**回滚 L1 并报告**（不同步半套内核）。
5. **离线可判定**：状态查询（落后 N 个提交、L2 漂移）只读上游 mirror，不做破坏性操作。

## 2. 三个上游与同步层级

| 上游 | remote | branch | pinned（初始） | 层级 | 同步内容 |
|------|--------|--------|----------------|------|----------|
| drawio | github.com/jgraph/drawio | `dev` | `9034053fb` | **L1** | `src/main/webapp/*` 整目录 → `public/drawio/` |
| next-ai-draw-io | github.com/DayuanJiang/next-ai-draw-io | `main` | `05adbe5` | **L2 + L3** | L2：system-prompt、shape-libraries/*.md、model-catalog.json（→ `src-tauri/assets/prompt/`）；L3：mcp-server/src 纯逻辑 + lib/{diagram-diff,diagram-pages}.ts → 报告映射到 drawio-core |
| drawio-to-visio | github.com/PanJitao/drawio-to-visio | `main` | `eed7363` | **L3** | `scripts/fix_svg_for_visio.py` → 报告映射到 `visio/converter.rs` |

> 完整机器可读定义见仓库根 `upstream.toml`（格式见 04 §5.1）。

## 3. 同步流水线

```
drawio-sync run [--all | <upstream-id>] [--dry-run]
┌─ 1. 解析 upstream.toml（schema 校验，错误即停）
├─ 2. 对每个上游：
│   ├─ mirror 确保存在（.sync/mirrors/<id>/，bare；不存在则 clone --bare）
│   ├─ git fetch origin <branch>
│   ├─ range = pinned_commit..origin/<branch>
│   │   ├─ range 为空 → 该上游 UP-TO-DATE
│   │   └─ 否则列出新 commit（sha + subject，上限 50 条显示）
├─ 3. 按层级应用（对 range 内的**最终** origin/<branch> 树操作，不是逐 commit 回放）：
│   ├─ L1（drawio）:
│   │   a. 回归前置检查：当前 vendor 的 embed 测试绿（本地已合入代码为准）
│   │   b. git tag pre-sync-<ts>（vendor+assets 全量）
│   │   c. 从 mirror 检出 <pinned 新值> 的 source_path 整目录 → 覆盖 public/drawio/
│   │      （本地有意修改在 sync/vendor-keep/patches/，覆盖后重放；apply 失败 → L1_BLOCKED）
│   │   d. 跑回归：tests/embed/ + 对齐测试 + visio 对齐测试
│   │      ├─ 全绿 → 继续
│   │      └─ 失败 → git reset 回 pre-sync tag → 报告 L1_BLOCKED（附失败摘要+新 commit 列表）
│   ├─ L2（文本资产）:
│   │   a. 抽取（TS 常量→md 用确定性抽取器，见 §4.2）→ 与目标文件 hash 比对
│   │   b. 变更 → 覆盖目标 + 记录 {path, hash_before, hash_after, 来源 commit}
│   │   c. L2 变更后跑 prompt 相关单测（组装不变性测试）
│   └─ L3（逻辑）:
│       a. 对每个 logic 映射：git diff pinned..HEAD -- <source_glob>
│       b. 生成报告条目 {source_file, maps_to, diff_stat, 变更 commit, 摘要}
│       c. 摘要 = diff 统计 + 变更函数/符号名提取（轻量解析：函数头/def 行）
├─ 4. 更新 upstream.toml 的 pinned_commit（仅对应用成功的上游；L1_BLOCKED 不推进）
├─ 5. 写报告 sync/reports/<ts>.md（schema 04 §5.2 的 markdown 渲染）
└─ 6. git：add 变更 → commit "chore(sync): <ts> drawio@<sha8>, next@<sha8>, visio@<sha8>" → tag sync-<ts>
```

**`--dry-run`**：1-3a 步为止（fetch + diff + 报告预览），不落盘、不推 pinned。

## 4. 关键子设计

### 4.1 L1 vendor 换版细节
- **整目录换版**而非文件级 merge：draw.io webapp 是编译产物耦合体（app.min.js 与源码必须同版本），文件级 merge 无意义且危险；
- `sync/vendor-keep/patches/`：本地对 webapp 的**有意修改**（如有）放此目录，换版覆盖后重新 apply（补丁清单形式：`*.patch`，apply 失败 → L1_BLOCKED，需人工处理）；
- 首次 vendor 即建立该机制（S2 任务 1），即使当前无本地修改（空清单）。

### 4.2 L2 抽取器（TS → md 的确定性转换）
- next-ai-draw-io 的 system prompt 是 TS 模板常量（`lib/system-prompts.ts` 的 DEFAULT_SYSTEM_PROMPT/STYLE_INSTRUCTIONS/MINIMAL_STYLE_INSTRUCTION）；
- 抽取器 = 固定脚本（drawio-sync 内 Rust 实现）：定位常量 → 去 TS 模板语法（`${...}` 保留为占位说明块）→ 输出 md，**头部注释**：来源文件 + 上游 commit + 抽取时间；
- **不变性测试**：对当前 pinned 版本，抽取结果必须与已入库 md 完全一致（防抽取器自身回归）；
- 若上游把 prompt 改成运行时拼接/多文件 → 抽取器失效 → 报告 `L2_EXTRACT_FAILED`，转人工（这是 L2 升 L3 的信号）。

### 4.3 L3 再移植工作流（人工环节，机制只负责「不出遗漏」）
1. 报告列出 {上游变更 → 影响的 Rust 模块}；
2. 开发者按条目移植 → 补/改对齐测试（S8/S9 的 fixture 机制）→ PR；
3. PR 合入后在报告的对应条目打勾（`sync/reports` 文件可编辑，或迁移到 issue 跟踪——v0.5 先用文件，v1.x 可接 issue）；
4. **未移植不影响 pinned 推进**（L3 不阻塞），但报告顶部常驻「未闭环 L3 条目」计数（NFR-5 可维护性指标）。

## 9. 纯手动兜底（最低运维流程）

drawio-sync 不可用（工具缺陷、环境缺 git 网络权限等）时，回退手动同步——语义与工具完全一致，本节即规范（drawio-sync 代码与测试以本节为准）：

1. **查看变更**：`git -C sample/<upstream> fetch origin && git -C sample/<upstream> log --oneline <pinned>..origin/<branch>`；
2. **L1（drawio）**：`git tag pre-sync-<ts>` → `git -C sample/drawio archive origin/dev src/main/webapp | tar -x -C /tmp/drawio-web` → 覆盖拷贝到 `public/drawio/`（保留目录内 LICENSE/NOTICE）→ 重放 `sync/vendor-keep/patches/*.patch` → 跑回归（tests/embed/ + 对齐测试 + visio 对齐）→ 失败则 `git reset --hard pre-sync-<ts>` 并停止；
3. **L2（文本资产）**：逐个打开变更的资产（system-prompt / shape-libraries / model-catalog）→ 更新到 `src-tauri/assets/prompt/`（保留来源头注释：来源文件 + 上游 commit + 抽取时间）→ 跑 prompt 不变性测试；
4. **L3（逻辑）**：人工读 diff → 移植对应 Rust 模块 + 更新对齐测试 → 在报告文件登记条目；
5. **收尾**：更新 `upstream.toml` pinned_commit → `git add -A && git commit -m "chore(sync): manual <ts> <摘要>" && git tag sync-<ts>`。

### 4.4 版本漂移语义
- L1 推进 = draw.io 内核升级（高风险，回归门禁把关）；
- L2 推进 = 提示词/资产升级（中风险，prompt 不变性测试 + AI 冒烟 1 次可选）；
- L3 推进 = 仅报告（风险由人工移植闭环）；
- 三者独立推进：`upstream.toml` 每个上游一个 pinned，互不牵连。

## 5. 实现载体

| 项 | 选择 | 理由 |
|----|------|------|
| 载体 | `src-tauri`（Rust bin `drawio-sync`） | 与 drawio-core 共享 hash/资产工具；可独立编译分发；CI 可直接用 |
| git 操作 | **shelling out 到系统 git**（不引 git2） | fetch/bare mirror/tag/diff 都是标准 git 语义；git2 对 bare mirror + partial clone 支持复杂且增依赖；git 是开发机/CI 必备 |
| mirror 缓存 | `.sync/mirrors/<id>/`（gitignore） | 避免每次全量 clone；`git fetch` 增量 |
| 调度 | 手动（SyncPanel 按钮 / CLI）+ 可选 CI `workflow_dispatch`/cron 每周 | Q4 默认手动；CI 只做「漂移检查」（不自动改主干，产出报告 PR 评论） |
| 并发/安全 | 单实例锁（`.sync/lock`）；fetch 只读上游；写仅 `public/drawio/`、`src-tauri/assets/prompt/`、`upstream.toml`、`sync/reports/` | 作用域最小化 |

## 6. 失败模式与处置

| 失败 | 处置 |
|------|------|
| fetch 失败（网络/权限） | 该上游 SKIPPED，其余继续；报告标注 |
| L1 回归失败 | 回滚 vendor，pinned 不推进，报告 L1_BLOCKED + 建议（看新 commit 是否触及 EditorUi/embed 协议） |
| L2 抽取失败 | 该资产 SKIPPED + L2_EXTRACT_FAILED（转人工/升 L3） |
| keep 补丁 apply 失败 | L1_BLOCKED（同回归失败路径） |
| pinned 指向的 commit 在上游不可达（force-push/rewind） | `drawio-sync doctor`：检测 pinned 不可达 → 提示重选 pinned（默认最新 tag/HEAD，人工确认） |
| 报告生成失败 | 不影响已应用内容；stderr 输出 JSON 原始结果兜底 |

## 7. 与 CI 的集成

- `ci.yml`：常规（每 PR）不含 sync；
- `sync-check.yml`（每周 + 手动）：在 runner 上 `drawio-sync run --dry-run --all` → 把 L2 漂移 + L3 变更摘要发 PR（对 `main` 的自动 PR）或 issue 评论；**永不自动 merge**；
- L1 真正换版永远发生在开发者的本地/受控流程（因为要跑全量回归并人工确认）。

## 8. 验收标准（对应 05 S10 / FR-S2）

1. 三上游 `drawio-sync run --all` 在当前 pinned 下报告 0 变更（幂等）；
2. pinned 回退 3 个 commit 后重跑：L2 资产正确覆盖（hash 变化入报告）、L3 条目生成、pinned 前推、tag/commit 生成、报告可读；
3. L1 阻塞演练：临时改坏 embed fixture → 换版 → 自动回滚 + L1_BLOCKED 报告；
4. `--dry-run` 不产生任何工作区变更（`git status` 干净）。
