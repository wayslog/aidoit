# Changelog

All notable changes to this project will be documented in this file.

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
