# aidoit

`aidoit` 是一个基于 Codex transcript 的本地图谱 CLI。

它的目标很直接：你回到一个仓库时，不需要重新翻聊天记录和零散笔记，直接运行命令，就能从 transcript 恢复当前主线、候选分支、原则、依赖和最小动作流。

## 当前能力

- 从 Codex transcript 增量导入事件，写入 SQLite `raw_events` 和读模型
- 输出主线任务、任务树、可执行 agenda、已确认原则和节点 inspect 历史
- 支持最小动作命令：`set-status`、`confirm`、`reject`、`promote`
- 列表视图会暴露短 ID，动作命令接受唯一前缀，方便从列表直接继续操作

## 命令

```bash
aidoit status
aidoit tree
aidoit agenda
aidoit principles
aidoit inspect <id-or-prefix>
aidoit set-status <id-or-prefix> <parked|ready|blocked|done|archived>
aidoit confirm <id-or-prefix>
aidoit reject <id-or-prefix>
aidoit promote <id-or-prefix>
```

常用全局参数：

```bash
aidoit --repo-root /path/to/repo --transcript /path/to/session.jsonl --db-path /path/to/aidoit.db status
```

如果不传 `--transcript`，CLI 会尝试从 `~/.codex/sessions/` 中找到当前仓库最近的一份 transcript。

## 开发

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

## 发布

- CI 会在 GitHub Actions 上执行格式检查、Clippy 和测试
- 推送 `v*` tag 后，会构建 Linux、macOS Intel、macOS Apple Silicon、Windows 二进制并发布到 GitHub Release

## 文档入口

- [CHANGELOG.md](CHANGELOG.md)
- [TODOS.md](TODOS.md)
- [实现计划](docs/superpowers/plans/2026-04-14-transcript-pipeline-v1.md)
