# 技术设计 / 实现笔记

## 事故记录:filter-repo 重写 tag 触发 release workflow(2026-06-15)

### 现象
pre-public-cleanup 任务执行 filter-repo 历史重写 + force push 后,
GitHub Actions 的 release.yml 被意外触发了 2 次,消耗了所剩无几的 GHA 额度(macOS 10x)。

### 根因
1. filter-repo 重写历史时,**所有 tag 指向的 commit hash 都变了**
   (v0.10.0/0.10.1/0.10.2 的 annotated tag 重新指向了重写后的 commit)。
2. `git push --force origin main` 虽然没显式带 `--tags`,
   但 GitHub 检测到了 tag 指向的变化(annotated tag 的 deref `v0.10.2^{}` 指向改变),
   按 `on: push: tags: v*` 触发了 release workflow。
3. 这是 force push + 历史重写的**必然副作用**,不是 filter-repo 的 bug。

### 教训(应沉淀进 spec)
**做 filter-repo / force push 之前,必须先处理 tag**:
- 方案 A(推荐):filter-repo 后、force push 前,先 `git push origin --delete` 删除所有远程 tag,
  force push 新历史后再用新 hash 重建 tag。这样不会触发"tag 变化"事件。
- 方案 B:临时把 release.yml 的触发条件改成 `workflow_dispatch` only,
  force push 完成后再改回来。
- 方案 C(最稳):force push 前在 GitHub 仓库设置里**临时禁用 release workflow**,
  push 完再启用。

**通用教训:任何涉及历史重写 + force push 的操作,都要先评估 CI 触发条件。**
tag-based 的 release workflow 对 tag 变化极其敏感。

### 损失
- 2 次 release workflow 运行(各 ~50 分钟 macOS universal build × 10x = 各 500 分钟)。
- GHA 额度彻底耗尽,7/1 刷新前无法用 GHA 发版。
- 两次 workflow 都失败了(tag 指向的 commit 已被重写,构建产物可能异常)。

### 补救
- 额度 7/1 刷新,届时可恢复。
- 发版临时方案:等额度刷新,或走其他 CI(CircleCI 仍在)。
- 远程 tag 状态:本地 tag 已是重写后 hash,远程还是旧 hash,
  后续需要同步(但同步会再次触发 workflow,必须等额度刷新或先禁用 workflow)。
