# 工时写入 commit 内容截断优化

## 问题

复盘页面写入工时（一键写入 / 手动写入）时，commit 标题被 `cleanCommitTitle` 截断到 60 字符（前端）或 80 字符（后端），导致写入禅道的工时内容丢失细节。

## 期望行为

- **UI 展示**：commit 列表可以截断（60 字符），保持界面整洁
- **写入内容**：不截断或大幅放宽限制（200 字符），保留完整信息
- **前后端一致**：统一截断长度，避免同一 commit 在不同路径下产生不同文本

## 改动范围

- `cleanCommitTitle.ts`：写入路径调用时传入更大的 maxLen
- `useReviewWriteHours.ts` / `ReviewWindow.vue`：buildWorkContent 用更大 maxLen
- `daily_review.rs`：build_default_work_content 统一 maxLen
- `BatchWriteApp.vue`：fallback 路径也要经过 cleanCommitTitle
