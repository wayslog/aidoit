# AGENTS Ship 前置校验 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 恢复仓库 `AGENTS.md`，并明确要求 `$ship` / `$gstack-ship` 在进入 ship skill 前先通过 CI 同口径本地校验。

**Architecture:** 这次不改业务代码，只改仓库协作约束。`AGENTS.md` 负责代理执行规则，spec/plan 文件负责把这次约束变化留档，便于后续维护。

**Tech Stack:** Markdown、GitHub Actions workflow 约束、gstack skill routing

---

### Task 1: 记录约束变更

**Files:**
- Create: `docs/superpowers/specs/2026-04-14-ship-precheck-agents-design.md`
- Create: `docs/superpowers/plans/2026-04-14-ship-precheck-agents.md`

- [ ] **Step 1: 写入短 spec，记录为什么要把 ship 前置校验写进 AGENTS.md**
- [ ] **Step 2: 写入短 plan，记录要恢复的规则和新增的 ship 约束**

### Task 2: 恢复并扩展 AGENTS.md

**Files:**
- Create: `AGENTS.md`
- Reference: `.github/workflows/test.yml`

- [ ] **Step 1: 恢复中文沟通与输出要求**
- [ ] **Step 2: 恢复 skill routing 规则**
- [ ] **Step 3: 新增 `$ship` / `$gstack-ship` 前置校验规则，命令与 CI 完全同口径**
- [ ] **Step 4: 明确任一前置校验失败时，不得继续进入 ship workflow**

### Task 3: 自检

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: 对照 `.github/workflows/test.yml`，确认命令完全一致**
- [ ] **Step 2: 检查 `AGENTS.md` 文案是否同时覆盖 `$ship` 和 `$gstack-ship`**
- [ ] **Step 3: 运行 `git diff --check`，确认没有格式问题**
