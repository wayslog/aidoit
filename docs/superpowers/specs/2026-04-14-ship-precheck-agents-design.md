# AGENTS Ship 前置校验设计

## 目标

恢复仓库根的 `AGENTS.md`，并把 `$ship` / `$gstack-ship` 的前置行为写成明确规则：在进入 ship skill 之前，必须先跑与 CI 完全同口径的本地校验。

## 设计范围

- 恢复原有中文沟通与输出约束
- 恢复原有 skill routing 规则
- 新增 ship 前置校验规则，直接引用当前 CI 命令
- 约定 `.github/workflows/test.yml` 是 ship 前置校验的事实来源

## 非目标

- 不改 ship skill 本体
- 不改业务代码
- 不新增额外 lint / build 命令，除非它们已经进入当前 CI 工作流

## 关键决策

### 1. 用 `AGENTS.md` 约束，而不是改 README

这是代理执行约束，不是用户说明文档。应该放在代理入口文件，而不是 README。

### 2. 前置校验与 CI 保持完全同口径

当前 `.github/workflows/test.yml` 的校验顺序是：

1. `cargo fmt --all --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test`

`AGENTS.md` 直接写死这三条命令，并说明如果 CI 命令将来变化，以 workflow 为准同步更新。

### 3. `$ship` / `$gstack-ship` 先验失败时不得继续

如果任一命令失败，不能直接进入 ship skill 流程。必须先报告失败、修复问题，再重新跑同口径校验。

## 目标文件

- `AGENTS.md`
- `docs/superpowers/specs/2026-04-14-ship-precheck-agents-design.md`
- `docs/superpowers/plans/2026-04-14-ship-precheck-agents.md`
