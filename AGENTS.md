# 沟通与输出要求

沟通和讨论的时候一律使用中文对话，包括 review 的输出结果。
生成的代码、注释或配置文件中一律使用中文进行输出。
尽量不使用 emoji，除非我显式的要求。

## Skill routing

当用户的请求匹配可用技能时，始终优先调用对应技能，而不是直接即兴回答。
先走专门流程，再进入普通对话或实现阶段。

关键路由规则：
- 产品想法、值不值得做、头脑风暴，优先调用 `office-hours`
- Bug、报错、为什么坏了、500 错误，优先调用 `investigate`
- Ship、deploy、push、创建 PR，优先调用 `ship`
- QA、测试站点、找 bug，优先调用 `qa`
- Code review、检查 diff，优先调用 `review`
- 发布后更新文档，优先调用 `document-release`
- 周回顾、总结本周交付，优先调用 `retro`
- 设计系统、品牌、视觉基调，优先调用 `design-consultation`
- 视觉审查、设计打磨，优先调用 `design-review`
- 架构评审、实现前技术评审，优先调用 `plan-eng-review`
- 保存进度、检查点、恢复上下文，优先调用 `checkpoint`
- 代码质量、健康检查，优先调用 `health`

## Ship 前置校验

当用户明确要求 `$ship` 或 `$gstack-ship` 时，在进入对应 skill workflow 之前，必须先运行与当前 CI 完全同口径的本地校验。

当前仓库的 ship 前置校验命令如下，顺序也必须一致：

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

执行规则：

- 三条命令必须全部通过，才能继续执行 `$ship` 或 `$gstack-ship`
- 任意一条失败时，不得继续进入 ship workflow
- 失败后应先报告是哪一条命令失败、核心错误是什么，再修复问题
- 修复完成后，必须重新完整运行这三条命令，不能只补跑失败的那一条就直接 ship
- 如果未来 `.github/workflows/test.yml` 里的 CI 校验命令发生变化，这里的前置校验也必须同步更新，并始终以 workflow 为事实来源
