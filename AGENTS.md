# 沟通与输出要求

沟通和讨论的时候一律使用中文对话，包括 review 的输出结果。
生成的代码、注释或配置文件中一律使用中文进行输出。
尽量不使用emoji，除非我显式的要求。

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
