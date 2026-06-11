# Journal - jarvis (Part 1)

> AI development session journal
> Started: 2026-06-02

---



## Session 1: 批量写工时窗口 + 今日计划工时估算与任务搜索

**Date**: 2026-06-02
**Task**: 批量写工时窗口 + 今日计划工时估算与任务搜索
**Branch**: `main`

### Summary

新建 BatchWriteApp 独立窗口，汇总当天提交按任务分组一键写入禅道；改造 TodayPlan 加工时估算、自定义/事务条目、全量任务搜索（修复候选截断为10条的问题）；禅道后端合并 my-task/my-work-task 双接口去重。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `3250e12` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 2: 设置页改造：单窗口 + 侧边栏，10页合并为6页

**Date**: 2026-06-02
**Task**: 设置页改造：单窗口 + 侧边栏，10页合并为6页
**Branch**: `main`

### Summary

SettingsDetailApp 改为左右侧边栏布局，190px 可折叠导航；日常提醒合并作息+主动提醒+定时提醒+今日覆盖共7个组件，外观与行为合并自启+名称+模式+宠物+点击共5个组件；旧 key 通过 LEGACY_PAGE_MAP 兼容映射；settings_open 用自定义事件替代 window.location.reload 实现 SPA 内导航。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `8111746` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 3: 项目成本分析 v3：数据源改帆软 + 渠道机器人接成本

**Date**: 2026-06-03
**Task**: 项目成本分析 v3：数据源改帆软 + 渠道机器人接成本
**Branch**: `main`

### Summary

成本分析数据源从禅道 OpenAPI 全面改为帆软 BI（reportIndex=1 工时明细），避免 N+1 打崩禅道。FineReport 客户端新增 new_with_timeout(60s) 和 submit_filter 的 project_name/user_status 参数。cost_rates 重写：帆软为唯一数据源，聚合 key 改员工中文名，不变式 总工时==正常+加班。渠道端（QQ/Telegram）message_handler 注册成本工具和触发词。前端 CostApp.vue 时间范围快捷档 + 含离职 checkbox。新增 .trellis/spec/backend/ 合约文档。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `627231a` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 4: Agent 记忆系统审查与改进

**Date**: 2026-06-03
**Task**: Agent 记忆系统审查与改进
**Branch**: `main`

### Summary

check agent 审查发现 6 个 bug（混合检索排序、嵌入维度截断、零向量降级、DB 初始化崩溃、tool_calls 污染、空响应处理）并全部修复；追加 conversation_id 透传和嵌入地址可配置两项改进

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `eb59b8b` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 5: 工时写入 commit 截断修复

**Date**: 2026-06-03
**Task**: 工时写入 commit 截断修复
**Branch**: `main`

### Summary

写入路径统一 maxLen=200（前端 buildWorkContent x2 + BatchWriteApp fallback x2 + 后端 build_default_work_content + evidence），UI 展示保持 60 不变

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `d08d1ae` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 6: 工时内容 AI 精简按钮

**Date**: 2026-06-03
**Task**: 工时内容 AI 精简按钮
**Branch**: `main`

### Summary

新增 summarize_work_content 命令，复盘页和一键写工时页 textarea 旁加精简按钮，点击调用 LLM 压缩 commit 记录后回填

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `bcd5c88` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 7: CC Switch 全量 provider 导入

**Date**: 2026-06-03
**Task**: CC Switch 全量 provider 导入
**Branch**: `main`

### Summary

扫描 CC Switch SQLite 全量 providers，按 Claude/Codex 分组展示，勾选批量导入为 llmProfiles，已导入自动去重

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `e52fc96` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 8: 成本与记忆模块代码审查与修复

**Date**: 2026-06-04
**Task**: 成本与记忆模块代码审查与修复
**Branch**: `main`

### Summary

审查昨日新增的 Agent 记忆系统与重构的项目成本分析，修复 4 严重+9 重要+若干建议项。成本：修复加班拆分参数 snake/camel 笔误致功能失效、区间起止校验、跨午夜班次阈值、preview 限窗防超时、清理死分支与死代码。记忆：提取改后台 spawn 异步化、修 FTS 外部内容表索引腐化、嵌入不可用降级纯 FTS+404 短路防首字延迟、相似事实不再覆盖、三表事务化、移除空转 core 子系统改取高 importance。cargo check / cargo test memory(18 passed) / vite build 均通过。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `02408b4` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 9: ui-theme M1 收尾归档 + 语音输入任务立项

**Date**: 2026-06-06
**Task**: ui-theme M1 收尾归档 + 语音输入任务立项
**Branch**: `main`

### Summary

修复极简主题下 UpdateWindow 悬浮面板透视（.update-panel 改用 --popup-bg）；theming.md 补「透明主窗面板背景必须用 --popup-bg」规则 + canvas 主题（matrix/cyber）组件 MatrixRain/CyberParticles 实现契约；提交 3cd7ee0 并推送 origin/main。ui-theme-redesign 经用户 GUI 拍板（确认 6 套主题、极简面板可见）后归档。另立 voice-input 任务存根（planning）：工具型语音转写+注入聚焦框、明确不做系统 IME，待 brainstorm 收敛 STT 本地/云端等问题。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `3cd7ee0` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 10: 轻量语音输入工具完成并归档

**Date**: 2026-06-11
**Task**: 轻量语音输入工具完成并归档
**Branch**: `main`

### Summary

轻量语音输入工具（语音转写+注入聚焦框，非系统输入法）完成并归档。后端核心：录音→ASR→注入聚焦框；首启自动下载引擎与模型（国内镜像 ghfast.top + 代理 + 断点续传/重试 + 手动兜底）；全局热键可自定义；引擎由 whisper-cli 切换为 sherpa-onnx + SenseVoice(FunASR)，并新增云端引擎（火山/豆包 ASR），本地/云端可选；转写优化（多线程提速、锁中文、术语词库、识别中反馈）；全链路诊断与失败可见化。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `73e2ab1` | (see git log) |
| `969d07e` | (see git log) |
| `ee9fe58` | (see git log) |
| `ad4949b` | (see git log) |
| `733a2ae` | (see git log) |
| `4f1a012` | (see git log) |
| `42b1cb4` | (see git log) |
| `5981001` | (see git log) |
| `6488b80` | (see git log) |
| `3395da2` | (see git log) |
| `16c64b0` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 11: 对话式发版打通：配置/连接体验重做 + 发版闭环 + 多项修复

**Date**: 2026-06-11
**Task**: 对话式发版打通：配置/连接体验重做 + 发版闭环 + 多项修复
**Branch**: `main`

### Summary

把对话式发版从配不通修到端到端可用。配置：设置页去掉「凭据名」内部概念（id 自动隐藏），账号卡片=用户名(=Jenkins User ID,带说明)/token/项目(job+别名必填)，Jenkins 地址全局；凭据加回 username（重构曾误删致 jenkins-mcp 起不来）。连接：测试连接改直连 Jenkins /api/json 验证当前填写值、不必先保存。发版闭环：prepare-deploy 两阶段（匹配 job → 读 get_job_info 参数 → 带 parameters 回调生成带按钮的确认卡片 → confirm-deploy → trigger_build）；jenkins toolPolicy 安全默认(写 confirm/只读 auto)，修只读 get_job_info 被默认 confirm 拦死。修复：spawn 加 CREATE_NO_WINDOW 消除 node 黑框；call_tool 僵尸自愈修 Transport closed 卡死；构建轮询取 lastBuild 修「查询出错」（queueId≠buildNumber）；删除对话改应用内二次确认。spec：mcp-client.md/mcp-deploy-confirm.md 记录上述新契约。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `662f31c` | (see git log) |
| `9fa1f79` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete
