# Transcript Pipeline V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 基于 Codex transcript 构建单仓库本地图谱闭环，完成 `raw_events -> projector -> 读模型 -> status/tree/agenda/principles/inspect -> 最小状态流转`。

**Architecture:** CLI 读命令统一先做 transcript 增量导入，再将归一化事件追加到 `raw_events`，由事务内 projector 更新 SQLite 读模型。领域层只定义节点、关系和状态机；存储层只负责 schema、事务和查询；ingest 只负责从 Codex transcript 解析出最小事件集合。

**Tech Stack:** Rust 2024、`clap`、`rusqlite`、`serde`、`serde_json`、`anyhow`、`thiserror`、`tempfile`

---

## 文件结构

- `src/main.rs`
  - CLI 入口。
- `src/lib.rs`
  - 暴露 domain、store、ingest、cli 模块。
- `src/domain/mod.rs`
  - 节点类型、关系类型、状态枚举、状态机校验。
- `src/store/mod.rs`
  - SQLite 打开、schema 初始化、事务边界。
- `src/store/schema.rs`
  - 建表与索引。
- `src/store/raw_events.rs`
  - `raw_events` 与 ingest checkpoint 读写。
- `src/store/read_models.rs`
  - 节点、关系、读模型查询。
- `src/ingest/mod.rs`
  - transcript 导入协调器。
- `src/ingest/codex.rs`
  - Codex transcript JSONL 解析。
- `src/ingest/extract.rs`
  - 从 message 文本抽取 phase/task/branch/principle/agreement/status 事件。
- `src/cli/mod.rs`
  - 命令编排与读前自动导入。
- `tests/domain_flow.rs`
  - 状态机和最小动作测试。
- `tests/store_projection.rs`
  - schema、幂等 raw_events、事务投影测试。
- `tests/ingest_codex.rs`
  - 真实 transcript 结构解析与抽取测试。
- `tests/cli_e2e.rs`
  - `status/tree/agenda/principles/inspect` 和最小动作闭环。
- `tests/fixtures/codex_session.jsonl`
  - 最小可读 transcript fixture。

### Task 1: 建立 crate 骨架与依赖

**Files:**
- Modify: `Cargo.toml`
- Create: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `cargo test`

- [ ] **Step 1: 补充依赖并暴露库入口**
- [ ] **Step 2: 运行 `cargo test`，确认骨架可编译**

### Task 2: 先写 domain 失败测试，再实现状态机

**Files:**
- Create: `tests/domain_flow.rs`
- Create: `src/domain/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: 为 Branch/Task 合法与非法迁移写失败测试**
- [ ] **Step 2: 为 Principle/Agreement `proposed -> confirmed/rejected` 写失败测试**
- [ ] **Step 3: 实现最小领域模型与状态机校验**
- [ ] **Step 4: 运行 `cargo test tests::domain_flow`，确认转绿**

### Task 3: 先写 store 失败测试，再实现 SQLite schema

**Files:**
- Create: `tests/store_projection.rs`
- Create: `src/store/mod.rs`
- Create: `src/store/schema.rs`
- Create: `src/store/raw_events.rs`
- Create: `src/store/read_models.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: 为 schema 初始化、checkpoint 保存、raw_events 幂等去重写失败测试**
- [ ] **Step 2: 为 projector 事务失败不留下半更新写失败测试**
- [ ] **Step 3: 实现 store 打开、schema 初始化、raw_events / checkpoint / 读模型接口**
- [ ] **Step 4: 运行 `cargo test store_projection`，确认转绿**

### Task 4: 先写 ingest 失败测试，再实现 Codex transcript 解析

**Files:**
- Create: `tests/fixtures/codex_session.jsonl`
- Create: `tests/ingest_codex.rs`
- Create: `src/ingest/mod.rs`
- Create: `src/ingest/codex.rs`
- Create: `src/ingest/extract.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: 用真实 `event_msg.user_message / agent_message` 结构写导入失败测试**
- [ ] **Step 2: 为重复导入、resume 增量、未知事件跳过写失败测试**
- [ ] **Step 3: 实现 transcript 解析、最小规则抽取与 raw_events 追加**
- [ ] **Step 4: 运行 `cargo test ingest_codex`，确认转绿**

### Task 5: 先写 CLI 失败测试，再实现读模型视图

**Files:**
- Create: `src/cli/mod.rs`
- Modify: `src/main.rs`
- Create: `tests/cli_e2e.rs`

- [ ] **Step 1: 为 `status/tree/agenda/principles/inspect` 写失败测试**
- [ ] **Step 2: 保证每个读命令都会先执行增量导入**
- [ ] **Step 3: 实现最小文本渲染，覆盖主线、阻塞、候选支线、原则和 inspect 历史**
- [ ] **Step 4: 运行 `cargo test cli_e2e`，确认转绿**

### Task 6: 先写动作命令失败测试，再实现最小状态流转

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/store/read_models.rs`
- Modify: `tests/cli_e2e.rs`
- Modify: `tests/domain_flow.rs`

- [ ] **Step 1: 为 Branch 状态更新、非法迁移报错、Principle/Agreement 确认写失败测试**
- [ ] **Step 2: 为 Branch 提升为独立 Task 后 `tree/inspect` 可见写失败测试**
- [ ] **Step 3: 实现动作命令和读模型更新**
- [ ] **Step 4: 运行目标测试并确认转绿**

### Task 7: 回归验证与整理

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/**/*.rs`
- Modify: `tests/**/*.rs`

- [ ] **Step 1: 运行 `cargo fmt`**
- [ ] **Step 2: 运行 `cargo test`**
- [ ] **Step 3: 运行 `cargo clippy --all-targets --all-features -- -D warnings`**
- [ ] **Step 4: 检查范围，确认没有引入 worktree、跨仓库、Web 控制台、多模型兼容**

## 自检

- 设计稿要求的主线是否都落在任务里：`transcript -> raw_events -> projector -> 读模型 -> 视图 -> 最小动作`
- 是否显式覆盖了测试计划中的关键路径：首次导入、重复导入、agenda 过滤、principles 过滤、非法迁移、事务一致性
- 是否没有把 defer 的事项偷渡进 schema、CLI 或抽象层
