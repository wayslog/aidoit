# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0.1] - 2026-04-15

### Fixed
- 修复 GitHub Release 工作流里的 Intel macOS runner 标签，避免打 tag 后因 `macos-13` 配置不受支持而中断发布。

## [0.2.0.0] - 2026-04-15

### Added
- 新增 execution-unit 原生模型，基于 `git common dir` 把主工作目录和 linked worktree 识别为同一 project 下的多个执行单元，并把节点/关系/事件作用域拆分为 project 级与 unit 级。
- 新增 `aidoit units`，直接展示当前 project 的 execution unit 摘要，包括 unit 类型、路径、branch、head、当前/激活标记和工作项统计。
- 新增 `aidoit global agenda --lens <urgent|easy|mainline|low-switch>`，基于同一份 `unit_summaries` 输出带排序依据说明的全局推荐列表。
- 新增 execution-unit 回归测试，覆盖主仓库 + linked worktree 识别、lens 排序差异、stale/duplicate worktree、旧 schema fail-fast 和 active 同步语义。

### Changed
- transcript 自动发现不再只按单仓库匹配，而是按 project 维度在 `~/.codex/sessions/` 中寻找最近 transcript，并按 transcript 自己所属的 execution unit 导入。
- `status`、`tree`、`agenda`、`principles`、`inspect`、`set-status`、`confirm`、`reject`、`promote` 继续可用，但语义已切到“当前 unit + project 级共享原则/协定”。
- 本地数据库路径改为 project 级共享，同一 project 下的多个 worktree 复用同一份 SQLite 数据。

### Fixed
- 默认 transcript 发现现在会跳过损坏 session 和 `cwd` 指向失效 worktree 的 stale session，不再把 CLI 默认入口直接打爆。
- `global agenda` 推荐项现在会显示可定位的 unit 路径，不再只有无法区分的 `linked_worktree` 标签。
- `easy` lens 的理由文案不再把未 ready 的主线 task 说成 ready。

## [0.1.0.0] - 2026-04-14

### Added
- 初版 `aidoit` CLI，提供 `status`、`tree`、`agenda`、`principles`、`inspect`、`set-status`、`confirm`、`reject`、`promote` 命令，能够直接从 Codex transcript 恢复单仓库项目态势。
- 基于 SQLite 的 `raw_events`、checkpoint、节点和关系读模型，实现 transcript 增量导入、事务投影和最小图谱查询。
- 覆盖领域状态机、投影事务、transcript 解析、CLI 读写闭环和短 ID 前缀解析的测试套件。
- GitHub Actions 持续集成与发布流水线，自动执行 `cargo fmt --all --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`，并在打 tag 时构建多平台二进制。

### Changed
- CLI 列表视图现在统一显示可复制的短 ID，`inspect`、`set-status`、`confirm`、`reject`、`promote` 也接受唯一前缀，命令链路从“看见列表”到“继续操作”已经打通。
- 仓库根目录解析会从当前目录向上发现 `.git`，在子目录启动时也会归一到正确仓库身份。

### Fixed
- 导入器现在会在事务内先投影节点、再投影状态和关系，避免同一条消息里“先写状态/依赖，后声明节点”时导入失败。
- 重复 `NodeCaptured` 事件不再把已经推进到 `ready`、`confirmed` 等状态的节点回退到默认捕获状态。
