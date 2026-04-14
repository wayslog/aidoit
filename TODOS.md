# TODOS

## Architecture

### 跨仓库图谱聚合与全局 Agenda

**What:** 支持跨仓库图谱聚合，并提供跨项目的全局 agenda 视图。

**Why:** 解锁个人总控室式的多项目工作流管理，但当前会显著放大 v1 的作用域和索引复杂度。

**Context:** 第一版已经明确为单仓库闭环，数据库仍保留 `repo_root` 扩展位。后续如果要做跨仓库聚合，需要重新定义全局优先级、原则作用域和跨仓库节点身份，不能在 v1 里偷渡。

**Effort:** L
**Priority:** P3
**Depends on:** 单仓库图谱、读模型和命令级恢复闭环稳定

### `worktree` 并行建议器

**What:** 基于依赖关系和隔离规则，为候选 Branch 提供 `worktree` 并行建议。

**Why:** 这是产品终局的一部分，但不是第一版闭环的必要条件，应该在图谱和状态机稳定后再引入。

**Context:** 当前已经决定 `worktree` 不是第一性能力，v1 只做单仓库图谱、视图和状态流转。第二阶段可以把 `worktree` 建议器做成 agenda 的增强，而不是自动调度器。

**Effort:** M
**Priority:** P2
**Depends on:** 单仓库图谱、状态机、agenda 规则、inspect 上下文稳定

## Interface

### Web 控制台与图形化视图

**What:** 基于稳定读模型增加 Web 控制台，以及甘特图、燃尽图、图谱剖面等图形化视图。

**Why:** 提升长期项目的可观测性和浏览效率，但不影响第一版“命令级恢复项目态势”的核心目标。

**Context:** 当前 CLI 已覆盖 `status/tree/agenda/principles/inspect` 五类核心视图。图形层属于第二阶段展示增强，应该建立在已经验证过的 CLI 语义之上，而不是反过来驱动核心模型。

**Effort:** L
**Priority:** P3
**Depends on:** 读模型稳定、CLI 语义稳定、核心查询接口成熟

## Integrations

### 多模型 / 多 IDE 适配层

**What:** 在 Codex-first 闭环稳定后，扩展到 Claude Code 或其他 AI 工作流入口。

**Why:** 为未来兼容更多工作流保留路线，但不能让第一版为了“通用性”先背上额外抽象。

**Context:** 当前已经明确 `transcript` 是权威源，未来多入口应通过适配器接入统一图谱，而不是推翻现有模型。第二阶段做的是“适配器扩展”，不是“重做内核”。

**Effort:** L
**Priority:** P4
**Depends on:** Codex-first 闭环稳定、事件模型稳定、导入与投影测试成熟

## Completed
- 暂无已完成项
