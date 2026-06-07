import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { STYLE_THEMES, DEFAULT_STYLE_THEME } from '../style-themes'

export interface WorkPeriod {
  start: string   // HH:MM
  end: string     // HH:MM
  label: string
}

export type CommitsRange =
  | 'today' | 'yesterday' | 'thisWeek' | 'lastWeek'
  | 'last7days' | 'last30days' | 'thisMonth' | 'all'

/** 左键单击小人时弹出的内容。默认任务列表；可改成今日复盘等 */
export type LeftClickAction = 'tasks' | 'review'
export type WorkStyle = 'focused' | 'multi' | 'transactional' | 'balanced'

export interface TodayPlan {
  date: string
  taskIds: string[]
}

export interface JarvisConfig {
  /** 助手显示名（用户可改）。默认 "Jarvis"；只影响 UI 文案、问候、写工时审计文本 */
  assistantName: string
  /** 助手对用户的称呼（用户可改）。默认 "主人"；用在问候、启动提示等亲昵语境 */
  userTitle: string
  workSchedule: {
    workDays: number[]      // 0=Sun, 1=Mon ... 6=Sat
    periods: WorkPeriod[]
  }
  notifications: {
    quietDuringLunch: boolean
    quietAfterWork: boolean
    quietOnWeekends: boolean
    morningGreeting: boolean
    todayPlanPromptEnabled: boolean
    todayPlanPromptTime: string
    eveningSummary: boolean
    eveningSummaryMinutesBefore: number
    eveningSummaryChannelNotify: boolean
    effortClosingCheck: boolean
    effortClosingMinutesAfterWork: number
    effortClosingTargetHours: number
    effortClosingRepeatMinutes: number
    effortClosingLatestTime: string
    effortClosingChannelNotify: boolean
    /** 上班时段定时小提示（喝水/起身/午饭/下班）总开关 */
    workdayNudges: boolean
    /** 周期性提示（喝水/起身交替）的间隔（分钟）。<30 视为关闭 */
    nudgeIntervalMinutes: number
  }
  override: {
    todayMode: 'normal' | 'overtime' | 'dayoff'
    todayModeSetOn: string   // YYYY-MM-DD
  }
  zentao: {
    baseUrl: string           // 如 http://zentao.example.com:9538/zentao
    account: string           // 用户的禅道账号；密码在 OS 密钥链里，不存这里
  }
  /** 工时统计（FineReport）：禅道工时通过帆软报表读，密码在 keychain */
  fineReport: {
    baseUrl: string           // 如 http://REDACTED_DOMAIN
    account: string           // 帆软用户名（多为禅道账号同名）
    realName: string          // 中文显示名 —— 用于按本人过滤工时，空则不查询
  }
  /** LLM 接入（默认 DeepSeek，OpenAI 兼容）。apiKey 由后端存 OS 密钥链，前端只保留占位符 */
  llm: {
    provider: 'deepseek' | 'openai' | 'custom'
    baseUrl: string           // 厂商根域名，客户端按 wireApi 拼端点
    model: string             // 如 deepseek-chat / deepseek-reasoner / gpt-4o
    apiKey: string
    /** 'chat'=/v1/chat/completions（默认）；'responses'=/v1/responses（Codex CLI 协议）；'anthropic'=/v1/messages（Claude 协议） */
    wireApi?: 'chat' | 'responses' | 'anthropic'
  }
  /** 已保存的 LLM 配置列表，方便快速切换 */
  llmProfiles: LlmProfile[]
  /** 当前激活的 llmProfile id；空串表示未绑定 */
  activeLlmProfileId: string
  channels: {
    autoStart: boolean
    telegram: {
      enabled: boolean
      botToken: string
      apiBaseUrl: string
      proxy: string
      allowChatIds: string[]
      notifyChatIds: string[]
    }
    qqbot: {
      enabled: boolean
      appId: string
      appSecret: string
      sandbox: boolean
      allowUserIds: string[]
      allowGroupIds: string[]
      notifyUserIds: string[]
      notifyGroupIds: string[]
    }
  }

  repoRoots: string[]         // 扫描 git 提交的本地代码根目录列表
  /** 任务窗口里 commits 关联取多大时间范围 —— 默认本周，'all' 走全量 */
  commitsRange: CommitsRange
  /** 左键单击小人弹什么。默认任务列表 */
  leftClickAction: LeftClickAction
  /** 选用的宠物形象 id（见 petManifest.ts）；默认 'robo'。形象不在列表时回退到默认 */
  petId: string
  /** 开机自启 */
  autoStartOnBoot: boolean
  /** 定时提醒列表 */
  reminders: ScheduledReminder[]
  workStyle: WorkStyle
  todayPlan: TodayPlan
  /** 右键菜单主题 id，对应 menu-themes.ts 中的 id */
  menuTheme: string
  /** 视觉风格主题 id，对应 style-themes.ts / style.css 的 [data-theme]；默认 'sci-fi' */
  styleTheme: string
  /** 项目成本分析功能开关，默认关闭 */
  costFeatureEnabled: boolean
  /** 对话式发版（Jenkins）功能开关，默认关闭。开启后「接入」里出现发版配置项 */
  deployEnabled: boolean
  /** 语音输入功能开关，默认关闭。开启后热键/点小人可语音转写并注入聚焦框（PR2 接 UI） */
  voiceInputEnabled: boolean
  /** 语音输入全局热键（Tauri accelerator，如 "CommandOrControl+Shift+Space"）。可在设置里录制自定义 */
  voiceHotkey: string
}

export interface ScheduledReminder {
  id: string
  /** 标准 cron 表达式（5段：分 时 日 月 周） */
  cron: string
  /** 提醒内容 */
  message: string
  enabled: boolean
  createdAt: number
}

export interface LlmProfile {
  id: string
  /** 用户自定义名称 */
  name: string
  provider: 'deepseek' | 'openai' | 'custom'
  baseUrl: string
  model: string
  apiKey: string
  wireApi?: 'chat' | 'responses' | 'anthropic'
}

const defaultConfig = (): JarvisConfig => ({
  assistantName: 'Jarvis',
  userTitle: '主人',
  workSchedule: {
    workDays: [1, 2, 3, 4, 5],
    periods: [
      { start: '08:00', end: '12:00', label: '上午' },
      { start: '14:00', end: '18:00', label: '下午' },
    ],
  },
  notifications: {
    quietDuringLunch: true,
    quietAfterWork: true,
    quietOnWeekends: true,
    morningGreeting: true,
    todayPlanPromptEnabled: true,
    todayPlanPromptTime: '09:10',
    eveningSummary: true,
    eveningSummaryMinutesBefore: 30,
    eveningSummaryChannelNotify: false,
    effortClosingCheck: true,
    effortClosingMinutesAfterWork: 10,
    effortClosingTargetHours: 8,
    effortClosingRepeatMinutes: 0,
    effortClosingLatestTime: '21:00',
    effortClosingChannelNotify: false,
    workdayNudges: true,
    nudgeIntervalMinutes: 60,
  },
  override: {
    todayMode: 'normal',
    todayModeSetOn: '',
  },
  zentao: { baseUrl: 'http://REDACTED_INTERNAL_IP:8989/zentao', account: '' },
  fineReport: { baseUrl: 'http://REDACTED_DOMAIN', account: '', realName: '' },
  llm: {
    provider: 'deepseek',
    baseUrl: 'https://api.deepseek.com',
    model: 'deepseek-chat',
    apiKey: '',
    wireApi: 'chat',
  },
  channels: {
    autoStart: false,
    telegram: { enabled: false, botToken: '', apiBaseUrl: 'https://api.telegram.org', proxy: '', allowChatIds: [], notifyChatIds: [] },
    qqbot: { enabled: false, appId: '', appSecret: '', sandbox: false, allowUserIds: [], allowGroupIds: [], notifyUserIds: [], notifyGroupIds: [] },
  },
  repoRoots: [],
  commitsRange: 'thisWeek',
  leftClickAction: 'tasks',
  petId: 'robo',
  autoStartOnBoot: true,
  reminders: [],
  llmProfiles: [],
  activeLlmProfileId: '',
  workStyle: 'balanced',
  todayPlan: { date: '', taskIds: [] },
  menuTheme: 'default',
  styleTheme: DEFAULT_STYLE_THEME,
  costFeatureEnabled: false,
  deployEnabled: false,
  voiceInputEnabled: false,
  voiceHotkey: 'CommandOrControl+Shift+Space',
})

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

export const useConfigStore = defineStore('config', () => {
  const config = ref<JarvisConfig>(defaultConfig())
  const loaded = ref(false)
  const showSettingsWindow = ref(false)
  let savingTimer: ReturnType<typeof setTimeout> | null = null
  const SECRET_PLACEHOLDER = '********'
  const isPlaceholder = (value?: string) => value === SECRET_PLACEHOLDER

  // 机器人写 reminders / 其他窗口写配置后前端从磁盘刷新，此时不要反写回去覆盖
  let suppressSave = false

  async function load() {
    try {
      const remote = await invoke<JarvisConfig>('config_load')
      // 老 settings.json 可能没有新增字段 —— 把缺的字段从默认值补齐，否则
      // 模板字符串 / 下拉绑定会拿到 undefined 报错。
      const defaults = defaultConfig()
      const merged: JarvisConfig = {
        ...defaults,
        ...remote,
        assistantName: (remote.assistantName ?? '').trim() || defaults.assistantName,
        userTitle: (remote.userTitle ?? '').trim() || defaults.userTitle,
        // notifications 是嵌套对象，浅合并会丢掉新字段 —— 显式合并
        notifications: { ...defaults.notifications, ...(remote.notifications ?? {}) },
        // zentao 同理，且要兜默认值：旧 settings.json 可能 baseUrl 为空串 ——
        // 这种情况下 wizard 应该看到默认内网地址而不是空，避免每次都让用户手填。
        zentao: {
          baseUrl: remote.zentao?.baseUrl?.trim() ? remote.zentao.baseUrl : defaults.zentao.baseUrl,
          account: remote.zentao?.account ?? defaults.zentao.account,
        },
        fineReport: {
          baseUrl: remote.fineReport?.baseUrl?.trim()
            ? remote.fineReport.baseUrl
            : defaults.fineReport.baseUrl,
          account: remote.fineReport?.account ?? defaults.fineReport.account,
          realName: remote.fineReport?.realName ?? defaults.fineReport.realName,
        },
        llm: {
          provider: remote.llm?.provider ?? defaults.llm.provider,
          baseUrl: remote.llm?.baseUrl?.trim() ? remote.llm.baseUrl : defaults.llm.baseUrl,
          model: remote.llm?.model?.trim() ? remote.llm.model : defaults.llm.model,
          apiKey: remote.llm?.apiKey ?? defaults.llm.apiKey,
          wireApi: (['responses', 'anthropic'] as const).includes(remote.llm?.wireApi as any)
            ? (remote.llm!.wireApi as 'responses' | 'anthropic')
            : 'chat',
        },
        channels: {
          autoStart: remote.channels?.autoStart ?? defaults.channels.autoStart,
          telegram: {
            ...defaults.channels.telegram,
            ...(remote.channels?.telegram ?? {}),
            allowChatIds: remote.channels?.telegram?.allowChatIds ?? defaults.channels.telegram.allowChatIds,
            notifyChatIds: remote.channels?.telegram?.notifyChatIds ?? defaults.channels.telegram.notifyChatIds,
          },
          qqbot: {
            ...defaults.channels.qqbot,
            ...(remote.channels?.qqbot ?? {}),
            allowUserIds: remote.channels?.qqbot?.allowUserIds ?? defaults.channels.qqbot.allowUserIds,
            allowGroupIds: remote.channels?.qqbot?.allowGroupIds ?? defaults.channels.qqbot.allowGroupIds,
            notifyUserIds: remote.channels?.qqbot?.notifyUserIds ?? defaults.channels.qqbot.notifyUserIds,
            notifyGroupIds: remote.channels?.qqbot?.notifyGroupIds ?? defaults.channels.qqbot.notifyGroupIds,
          },
        },
        commitsRange: remote.commitsRange ?? defaults.commitsRange,
        leftClickAction: remote.leftClickAction === 'review' ? 'review' : defaults.leftClickAction,
        petId: (remote.petId ?? '').trim() || defaults.petId,
        autoStartOnBoot: remote.autoStartOnBoot ?? defaults.autoStartOnBoot,
        reminders: Array.isArray(remote.reminders) ? remote.reminders : defaults.reminders,
        llmProfiles: Array.isArray(remote.llmProfiles) ? remote.llmProfiles : defaults.llmProfiles,
        activeLlmProfileId: remote.activeLlmProfileId ?? defaults.activeLlmProfileId,
        workStyle: ['focused', 'multi', 'transactional', 'balanced'].includes(remote.workStyle)
          ? remote.workStyle
          : defaults.workStyle,
        menuTheme: remote.menuTheme ?? defaults.menuTheme,
        styleTheme: STYLE_THEMES.some(t => t.id === remote.styleTheme)
          ? remote.styleTheme
          : defaults.styleTheme,
        costFeatureEnabled: remote.costFeatureEnabled ?? defaults.costFeatureEnabled,
        deployEnabled: remote.deployEnabled ?? defaults.deployEnabled,
        voiceInputEnabled: remote.voiceInputEnabled ?? defaults.voiceInputEnabled,
        voiceHotkey: (remote.voiceHotkey ?? '').trim() || defaults.voiceHotkey,
        todayPlan: {
          date: remote.todayPlan?.date ?? defaults.todayPlan.date,
          taskIds: Array.isArray(remote.todayPlan?.taskIds)
            ? remote.todayPlan.taskIds
            : defaults.todayPlan.taskIds,
        },
      }
      // 临时覆盖只在当日有效
      if (merged.override.todayModeSetOn !== todayStr()) {
        merged.override.todayMode = 'normal'
        merged.override.todayModeSetOn = ''
      }
      // load() 替换 config.value 会触发 deep watcher → save → emit config-changed → 死循环。
      // 用 suppressSave 打断：watcher 检测到 suppressSave 就跳过 save。
      suppressSave = true
      config.value = merged
      Promise.resolve().then(() => { suppressSave = false })
    } catch (e) {
      console.error('加载配置失败，使用默认值:', e)
    } finally {
      loaded.value = true
    }
  }

  async function save() {
    try {
      await invoke('config_save', { config: config.value })
      if (config.value.llm.apiKey && !isPlaceholder(config.value.llm.apiKey)) {
        config.value.llm.apiKey = SECRET_PLACEHOLDER
      }
      if (config.value.channels.telegram.botToken && !isPlaceholder(config.value.channels.telegram.botToken)) {
        config.value.channels.telegram.botToken = SECRET_PLACEHOLDER
      }
      if (config.value.channels.qqbot.appSecret && !isPlaceholder(config.value.channels.qqbot.appSecret)) {
        config.value.channels.qqbot.appSecret = SECRET_PLACEHOLDER
      }
    } catch (e) {
      console.error('保存配置失败:', e)
    }
  }

  function applyRemote(remote: Partial<JarvisConfig>, fields: (keyof JarvisConfig)[]) {
    suppressSave = true
    for (const key of fields) {
      if (key in remote) {
        (config.value as any)[key] = remote[key]
      }
    }
    Promise.resolve().then(() => { suppressSave = false })
  }

  async function refreshReminders() {
    try {
      const remote = await invoke<JarvisConfig>('config_load')
      applyRemote(remote, ['reminders'])
    } catch (e) {
      console.error('刷新提醒列表失败:', e)
    }
  }

  async function applyLlmProfile(remote: JarvisConfig) {
    applyRemote(remote, ['llm', 'llmProfiles', 'activeLlmProfileId'])
  }

  // 任意字段变化 250ms 防抖后写回磁盘
  watch(config, () => {
    if (!loaded.value || suppressSave) return
    if (savingTimer) clearTimeout(savingTimer)
    savingTimer = setTimeout(save, 250)
  }, { deep: true })

  // 机器人通过 channels 写入提醒后，Rust 端 emit 此事件
  listen('reminders-changed', () => { refreshReminders() }).catch(() => {})

  // 其他窗口（设置窗口等）修改配置后，Rust 端 emit 此事件，主窗口需要刷新
  listen('config-changed', () => { load() }).catch(() => {})

  // 临时覆盖：今晚加班 / 今天休假
  function setTodayMode(mode: JarvisConfig['override']['todayMode']) {
    config.value.override.todayMode = mode
    config.value.override.todayModeSetOn = mode === 'normal' ? '' : todayStr()
  }

  // —— 派生：当前时间上下文 ——
  // 用一个 reactive tick 让计算属性每分钟刷新
  const tick = ref(Date.now())
  const _tickTimer = setInterval(() => { tick.value = Date.now() }, 30 * 1000)
  // Pinia setup store 是应用级单例，_tickTimer 生命周期与 app 等同，无需显式清理。

  function parseHm(hm: string): number {
    const [h, m] = hm.split(':').map(Number)
    return h * 60 + m
  }

  type Phase = 'before-work' | 'working' | 'lunch' | 'after-work' | 'weekend' | 'dayoff' | 'overtime'

  const phase = computed<Phase>(() => {
    void tick.value   // 触发响应
    const now = new Date()
    const todayMode = config.value.override.todayMode

    if (todayMode === 'dayoff') return 'dayoff'
    if (todayMode === 'overtime') {
      // 加班模式：把下班时间往后推 2 小时
    }

    const dow = now.getDay()
    const isWorkDay = config.value.workSchedule.workDays.includes(dow)
    if (!isWorkDay && todayMode !== 'overtime') return 'weekend'

    const minutes = now.getHours() * 60 + now.getMinutes()
    const periods = config.value.workSchedule.periods
      .map(p => ({ start: parseHm(p.start), end: parseHm(p.end) }))
      .sort((a, b) => a.start - b.start)

    if (periods.length === 0) return 'after-work'

    if (minutes < periods[0].start) return 'before-work'

    for (let i = 0; i < periods.length; i++) {
      if (minutes >= periods[i].start && minutes < periods[i].end) return 'working'
      if (i < periods.length - 1 && minutes >= periods[i].end && minutes < periods[i + 1].start) return 'lunch'
    }

    // 全部时段都已过：下班
    const lastEnd = periods[periods.length - 1].end
    if (todayMode === 'overtime' && minutes < lastEnd + 120) return 'working'
    return 'after-work'
  })

  const isQuietHours = computed(() => {
    const p = phase.value
    const n = config.value.notifications
    if (p === 'weekend' && n.quietOnWeekends) return true
    if (p === 'lunch' && n.quietDuringLunch) return true
    if ((p === 'after-work' || p === 'before-work') && n.quietAfterWork) return true
    if (p === 'dayoff') return true
    return false
  })

  // 离下班还有多少分钟（负数=已下班；上午/午休/下班前都返回到"今天最后一个时段结束"的差值）
  const minutesUntilEndOfDay = computed<number>(() => {
    void tick.value
    const now = new Date()
    const periods = config.value.workSchedule.periods
    if (periods.length === 0) return -1
    // 防御性排序：用户在设置里可能把 periods 顺序写乱，直接取下标 last 会把"下午到 18:00"
    // 误判成"上午到 12:00"，导致复盘提醒永远不触发或在错误时间触发。
    const sortedEnds = periods.map(p => parseHm(p.end)).sort((a, b) => a - b)
    const lastEnd = sortedEnds[sortedEnds.length - 1]
    const m = now.getHours() * 60 + now.getMinutes()
    return lastEnd - m
  })

  // 一个工作日的工时（所有 periods 时长之和）
  const hoursPerWorkDay = computed<number>(() => {
    let mins = 0
    for (const p of config.value.workSchedule.periods) {
      mins += parseHm(p.end) - parseHm(p.start)
    }
    return mins / 60
  })

  // 未来 N 天内累计可用工时（按 workDays 排除休息日，含今天）
  function availableHoursInNextDays(n: number): number {
    void tick.value
    const days = config.value.workSchedule.workDays
    const today = new Date()
    const todayMode = config.value.override.todayMode
    let hours = 0
    for (let i = 0; i < n; i++) {
      const d = new Date(today)
      d.setDate(today.getDate() + i)
      const dow = d.getDay()
      if (i === 0) {
        // 今天有 override 优先级
        if (todayMode === 'dayoff') continue
        if (days.includes(dow) || todayMode === 'overtime') {
          hours += hoursPerWorkDay.value
        }
        continue
      }
      if (days.includes(dow)) hours += hoursPerWorkDay.value
    }
    return hours
  }

  const availableHoursIn7Days = computed(() => availableHoursInNextDays(7))

  // 未来 N 天内的工作日个数
  function workingDaysInNext(n: number): number {
    const days = config.value.workSchedule.workDays
    const today = new Date()
    const todayMode = config.value.override.todayMode
    let count = 0
    for (let i = 0; i < n; i++) {
      const d = new Date(today)
      d.setDate(today.getDate() + i)
      const dow = d.getDay()
      if (i === 0) {
        if (todayMode === 'dayoff') continue
        if (days.includes(dow) || todayMode === 'overtime') count++
        continue
      }
      if (days.includes(dow)) count++
    }
    return count
  }

  const workingDaysIn7 = computed(() => workingDaysInNext(7))

  return {
    config,
    loaded,
    showSettingsWindow,
    load,
    save,
    refreshReminders,
    applyLlmProfile,
    setTodayMode,
    phase,
    isQuietHours,
    minutesUntilEndOfDay,
    hoursPerWorkDay,
    availableHoursIn7Days,
    workingDaysIn7,
    availableHoursInNextDays,
    workingDaysInNext,
  }
})
