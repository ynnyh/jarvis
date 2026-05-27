import type { Component } from 'vue'
import AssistantNameSection from './components/settings/AssistantNameSection.vue'
import ChannelsSection from './components/settings/ChannelsSection.vue'
import ExcludedLinesSection from './components/settings/ExcludedLinesSection.vue'
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
import ZentaoSection from './components/settings/ZentaoSection.vue'

export type SettingsPageKey = 'zentao' | 'ai' | 'channels' | 'code' | 'schedule' | 'nudges' | 'appearance'

export interface SettingsMenuItem {
  key: SettingsPageKey
  title: string
  desc: string
  group: string
}

export const SETTINGS_MENU: SettingsMenuItem[] = [
  { key: 'zentao', title: '禅道', desc: '任务读取、账号和密码', group: '接入' },
  { key: 'ai', title: 'AI 模型', desc: '服务商、模型、API Key', group: '接入' },
  { key: 'channels', title: '聊天渠道', desc: 'Telegram、QQ Bot', group: '接入' },
  { key: 'code', title: '代码与日报', desc: '仓库目录和业务线过滤', group: '工作流' },
  { key: 'schedule', title: '作息规则', desc: '工作日、时段、静默', group: '提醒' },
  { key: 'nudges', title: '主动提醒', desc: '仪式、小提示、今日覆盖', group: '提醒' },
  { key: 'appearance', title: '外观与行为', desc: '称呼、形象、点击动作', group: '个性化' },
]

export const SETTINGS_PAGE_COMPONENTS: Record<SettingsPageKey, Component[]> = {
  zentao: [ZentaoSection],
  ai: [LlmSection],
  channels: [ChannelsSection],
  code: [RepoRootsSection, ExcludedLinesSection],
  schedule: [WorkDaysSection, WorkPeriodsSection, QuietRulesSection],
  nudges: [RitualsSection, WorkdayNudgesSection, TodayOverrideSection],
  appearance: [AssistantNameSection, PetSection, LeftClickActionSection],
}

