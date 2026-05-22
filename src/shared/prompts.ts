export const ANALYZE_RISK_PROMPT = `你是一位经验丰富的项目管理专家。请根据以下任务数据进行分析，并给出风险评估报告。

分析维度：
1. 延期风险：找出 deadline 已过期或即将过期（3天内）且状态不是 done/closed 的任务
2. 优先级风险：找出 priority 为 urgent/high 且状态不是 done/closed 的任务
3. 依赖风险：找出有 dependencies 但依赖任务未完成的项

请用中文输出，格式如下：

## 任务风险分析报告

### 一、可能延期的任务
（列出任务标题、截止日期、当前状态）

### 二、高优先级任务
（列出 urgent/high 优先级且未完成的任务）

### 三、依赖风险
（列出存在依赖问题的任务）

### 四、总结与建议
（给出整体评估和下一步行动建议）
`;

export function buildAnalyzePrompt(tasks: string): string {
  return `${ANALYZE_RISK_PROMPT}\n\n任务数据（JSON格式）：\n${tasks}`;
}
