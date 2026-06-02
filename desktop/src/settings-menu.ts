import type { Component } from 'vue'
import AutoStartSection from './components/settings/AutoStartSection.vue'
import AssistantNameSection from './components/settings/AssistantNameSection.vue'
import RemindersSection from './components/settings/RemindersSection.vue'
import ChangelogSection from './components/settings/ChangelogSection.vue'
import ChannelsSection from './components/settings/ChannelsSection.vue'
import EffortClosingSection from './components/settings/EffortClosingSection.vue'
import ExcludedLinesSection from './components/settings/ExcludedLinesSection.vue'
import FineReportSection from './components/settings/FineReportSection.vue'
import LeftClickActionSection from './components/settings/LeftClickActionSection.vue'
import LlmSection from './components/settings/LlmSection.vue'
import PetSection from './components/settings/PetSection.vue'
import QuietRulesSection from './components/settings/QuietRulesSection.vue'
import RepoRootsSection from './components/settings/RepoRootsSection.vue'
import RitualsSection from './components/settings/RitualsSection.vue'
import TodayOverrideSection from './components/settings/TodayOverrideSection.vue'
import WorkDaysSection from './components/settings/WorkDaysSection.vue'
import WorkPeriodsSection from './components/settings/WorkPeriodsSection.vue'
import WorkdayNudgesSection from './components/settings/WorkdayNudgesSection.vue'
import WorkStyleSection from './components/settings/WorkStyleSection.vue'
import ZentaoSection from './components/settings/ZentaoSection.vue'

export type SettingsPageKey = 'zentao' | 'finereport' | 'ai' | 'channels' | 'code' | 'dailyNudges' | 'effortClosing' | 'personalization' | 'about'

export interface SettingsMenuItem {
  key: SettingsPageKey
  title: string
  desc: string
  group: string
  /** 侧边栏分组排序，越小越靠前 */
  groupOrder: number
}

export const SETTINGS_MENU: SettingsMenuItem[] = [
  { key: 'zentao', title: '禅道', desc: '任务读取、账号和密码', group: '接入', groupOrder: 1 },
  { key: 'finereport', title: '工时统计', desc: '查询与汇总个人工时', group: '接入', groupOrder: 1 },
  { key: 'ai', title: 'AI 模型', desc: '服务商、模型与 API Key', group: '接入', groupOrder: 1 },
  { key: 'channels', title: '聊天渠道', desc: 'Telegram 与 QQ Bot', group: '接入', groupOrder: 1 },
  { key: 'code', title: '代码与日报', desc: '仓库目录和业务线过滤', group: '工作流', groupOrder: 2 },
  { key: 'dailyNudges', title: '日常提醒', desc: '问候、计划、作息、静音和定时提醒', group: '提醒', groupOrder: 3 },
  { key: 'effortClosing', title: '工时提醒', desc: '下班后检查今日工时', group: '提醒', groupOrder: 3 },
  { key: 'personalization', title: '外观与行为', desc: '称呼、形象、工作模式与交互', group: '个性化', groupOrder: 4 },
  { key: 'about', title: '关于与更新', desc: '版本信息与历史更新日志', group: '关于', groupOrder: 5 },
]

export const SETTINGS_PAGE_COMPONENTS: Record<SettingsPageKey, Component[]> = {
  zentao: [ZentaoSection],
  finereport: [FineReportSection],
  ai: [LlmSection],
  channels: [ChannelsSection],
  code: [RepoRootsSection, ExcludedLinesSection],
  dailyNudges: [WorkDaysSection, WorkPeriodsSection, QuietRulesSection, RitualsSection, WorkdayNudgesSection, TodayOverrideSection, RemindersSection],
  effortClosing: [EffortClosingSection],
  personalization: [AutoStartSection, AssistantNameSection, WorkStyleSection, PetSection, LeftClickActionSection],
  about: [ChangelogSection],
}

/** 保留旧 key 到新 key 的映射，兼容 `settings_open(page='schedule')` 等旧调用 */
export const LEGACY_PAGE_MAP: Record<string, SettingsPageKey> = {
  schedule: 'dailyNudges',
  nudges: 'dailyNudges',
  appearance: 'personalization',
}
