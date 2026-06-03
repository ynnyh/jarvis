# 工时内容 AI 精简按钮

## 需求

在复盘页面的工时写入表单中，textarea 旁边加一个小按钮（如 "✨精简"），点击后调用 LLM 对当前工作内容进行提炼压缩，结果回填到 textarea。

## 交互设计

- 按钮位置：textarea 右上角或标签行，小图标按钮，不抢眼
- 点击后按钮变 loading 状态，textarea 禁用
- 成功后替换 textarea 内容
- 失败时 toast 提示，内容不变
- 如果 textarea 为空，不触发

## 技术方案

### 后端

新增 Tauri command `summarize_work_content`：
- 输入：`text: String`
- 输出：`String`（精简后的文本）
- 用现有 `llm::chat()` 非流式调用
- Prompt：要求将多条 commit 工作记录压缩为简洁的工时描述，保留关键信息，去掉冗余，控制在 200 字以内

### 前端

- ReviewWindow.template.html：textarea 旁加按钮
- ReviewWindow.vue：加 `summarizeContent` 函数，调用 invoke
- BatchWriteApp.vue：同样加按钮（如果也有 textarea 的话）
- style.css：按钮样式
