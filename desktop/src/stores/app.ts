import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export interface Task {
  id: string
  title: string
  description: string
  status: 'wait' | 'doing' | 'done' | 'closed' | 'cancel'
  priority: 'low' | 'normal' | 'high' | 'urgent'
  estimatedHours: number
  consumedHours: number
  deadline: string
  assignee: string
}

export interface RiskAnalysis {
  overdueTasks: Task[]
  highPriorityTasks: Task[]
  dependencyRisks: Array<{
    taskId: string
    taskTitle: string
    missingDependencies: string[]
    reason: string
  }>
  summary: string
}

export interface TaskAlert {
  id: string
  title: string
  deadline: string
  assignee: string
  alertType: 'overdue' | 'today' | 'soon' | 'upcoming'
  daysUntilDue: number
  status: 'wait' | 'doing'
  priority: 'low' | 'normal' | 'high' | 'urgent'
  estimatedHours: number
  consumedHours: number
  leftHours: number       // 禅道独立维护的剩余工时（团队任务=我个人 left）
  isTeam: boolean         // 团队任务（我在 team 中），工时是我个人的
}

export type AlertLevel = 'danger' | 'warning' | 'safe'

export interface DayStack {
  date: string          // YYYY-MM-DD
  daysFromToday: number
  count: number
  tasks: TaskAlert[]
}

export const useAppStore = defineStore('app', () => {
  const tasks = ref<Task[]>([])
  const todayTasks = ref<Task[]>([])
  const riskAnalysis = ref<RiskAnalysis | null>(null)
  const loading = ref(false)
  const showMenu = ref(false)
  const showTaskWindow = ref(false)
  const showAnalyzeWindow = ref(false)
  const toasts = ref<Array<{ id: number; title: string; message: string }>>([])

  // 任务提醒
  const taskAlerts = ref<TaskAlert[]>([])
  const alertsLoaded = ref(false)
  const alertsLastError = ref<string | null>(null)
  const overdueTasks = computed(() => taskAlerts.value.filter(a => a.alertType === 'overdue'))
  const todayAlertTasks = computed(() => taskAlerts.value.filter(a => a.alertType === 'today'))
  const soonTasks = computed(() => taskAlerts.value.filter(a => a.alertType === 'soon'))
  const upcomingTasks = computed(() => taskAlerts.value.filter(a => a.alertType === 'upcoming'))
  const overdueCount = computed(() => overdueTasks.value.length)
  const todayCount = computed(() => todayAlertTasks.value.length)
  const soonCount = computed(() => soonTasks.value.length)
  const upcomingCount = computed(() => upcomingTasks.value.length)

  // 堆叠检测：未来 7 天内同一天 ≥ 2 个 deadline 的日期
  const STACK_THRESHOLD = 2
  const stackedDays = computed<DayStack[]>(() => {
    const byDate = new Map<string, TaskAlert[]>()
    for (const a of taskAlerts.value) {
      if (a.daysUntilDue < 0) continue   // 逾期不算"堆叠未来风险"
      if (!byDate.has(a.deadline)) byDate.set(a.deadline, [])
      byDate.get(a.deadline)!.push(a)
    }
    const stacks: DayStack[] = []
    for (const [date, list] of byDate) {
      if (list.length >= STACK_THRESHOLD) {
        stacks.push({
          date,
          daysFromToday: list[0].daysUntilDue,
          count: list.length,
          tasks: list,
        })
      }
    }
    return stacks.sort((a, b) => a.daysFromToday - b.daysFromToday)
  })

  const alertLevel = computed<AlertLevel>(() => {
    if (overdueCount.value > 0 || todayCount.value > 0) return 'danger'
    if (soonCount.value > 0 || stackedDays.value.length > 0) return 'warning'
    return 'safe'
  })

  // ===== 风险分析派生数据 =====
  const showRiskWindow = ref(false)
  const showUpdateWindow = ref(false)

  interface UrgencyEntry {
    alert: TaskAlert
    score: number
    reasons: string[]
    leftHours: number
  }

  // 紧迫度评分：综合 deadline / 优先级 / 剩余工时 / 状态
  // 分值范围大约 0-200，>120 红色高危
  const urgencyScored = computed<UrgencyEntry[]>(() => {
    const PRIORITY_BONUS: Record<TaskAlert['priority'], number> = {
      urgent: 25, high: 10, normal: 0, low: -10,
    }
    return taskAlerts.value
      .map(a => {
        const reasons: string[] = []
        let score = 0

        // 基础：deadline 越近分数越高
        if (a.daysUntilDue < 0) {
          score += 100 + Math.min(60, -a.daysUntilDue * 3)
          reasons.push(`已逾期 ${-a.daysUntilDue} 天`)
        } else if (a.daysUntilDue === 0) {
          score += 100
          reasons.push('今天到期')
        } else {
          score += Math.max(0, 100 - a.daysUntilDue * 12)
          if (a.daysUntilDue <= 3) reasons.push(`${a.daysUntilDue} 天后到期`)
        }

        // 优先级
        const pBonus = PRIORITY_BONUS[a.priority] ?? 0
        if (pBonus > 0) {
          score += pBonus
          reasons.push(`${a.priority === 'urgent' ? '紧急' : '高'}优先级`)
        } else if (pBonus < 0) {
          score += pBonus
        }

        // 剩余工时（用禅道独立维护的 left 字段）
        const leftHours = a.leftHours
        if (leftHours > 16) {
          score += 20
          reasons.push(`你还有 ${leftHours}h 没完成`)
        } else if (leftHours > 8) {
          score += 10
          reasons.push(`你还有 ${leftHours}h 没完成`)
        }

        // 还没开始
        if (a.status === 'wait') {
          score += 8
          reasons.push('还没开始')
        }

        return { alert: a, score: Math.round(score), reasons, leftHours }
      })
      .sort((a, b) => b.score - a.score)
  })

  // 我剩余总工时（仅 7 天内的紧迫任务）
  const myRemainingHours = computed(() =>
    urgencyScored.value.reduce((sum, e) => sum + e.leftHours, 0)
  )

  // 7 天内分日剩余工时（用于"哪天压力大"）
  const hoursByDay = computed<Array<{ date: string, daysFromToday: number, hours: number, count: number }>>(() => {
    const map = new Map<string, { hours: number, count: number, daysFromToday: number }>()
    for (const e of urgencyScored.value) {
      const d = e.alert.deadline
      if (!map.has(d)) map.set(d, { hours: 0, count: 0, daysFromToday: e.alert.daysUntilDue })
      const slot = map.get(d)!
      slot.hours += e.leftHours
      slot.count += 1
    }
    return Array.from(map.entries())
      .map(([date, v]) => ({ date, ...v }))
      .sort((a, b) => a.daysFromToday - b.daysFromToday)
  })

  // ===== 任务-提交关联 =====

  interface CommitLink {
    sha: string
    shortSha: string
    title: string
    authoredDate: string
    repoPath: string
    businessLine: string
    repoName: string
    matchType: 'exact' | 'soft'
    matchedKeywords?: string[]
    effort: number
  }

  /** taskId → 该任务关联的 commit 列表 */
  const commitsByTask = ref<Record<string, CommitLink[]>>({})
  const commitsLoaded = ref(false)
  const commitsLastError = ref<string | null>(null)
  const commitsRange = ref<string>('thisWeek')
  /** 用户对 (taskId, sha) 的反馈：accepted / rejected；rejected 的 commit 不再展示 */
  const COMMIT_FEEDBACK_STORAGE_KEY = 'jarvis.commitFeedback.v1'
  const loadStoredFeedback = (): Record<string, 'accepted' | 'rejected'> => {
    try {
      const raw = localStorage.getItem(COMMIT_FEEDBACK_STORAGE_KEY)
      if (!raw) return {}
      const parsed = JSON.parse(raw)
      return typeof parsed === 'object' && parsed !== null ? parsed : {}
    } catch {
      return {}
    }
  }
  const commitFeedback = ref<Record<string, 'accepted' | 'rejected'>>(loadStoredFeedback())
  watch(commitFeedback, (v) => {
    try {
      localStorage.setItem(COMMIT_FEEDBACK_STORAGE_KEY, JSON.stringify(v))
    } catch {
      // localStorage 满了 / 不可用，忽略
    }
  }, { deep: true })

  // ===== 任务绑定流程 =====
  // 待处理的新任务队列（由后端 "new-tasks-detected" 事件填入）。
  // 绑定窗每次只处理队首一个任务，处理完 shift 一个出来；为空就关窗。
  // 用 ref + 显式 push/shift 而非数组直接 mutate，确保 Vue 响应式触发。

  interface PendingBindTask {
    id: string
    title: string
    priority: string
    deadline: string
  }
  const pendingBindTasks = ref<PendingBindTask[]>([])
  const showBindTaskWindow = ref(false)

  function enqueueBindTask(t: PendingBindTask) {
    // 去重：同 id 已在队列中就跳过
    if (pendingBindTasks.value.some(x => x.id === t.id)) return
    pendingBindTasks.value.push(t)
  }
  function dequeueBindTask(): PendingBindTask | null {
    return pendingBindTasks.value.shift() ?? null
  }

  // 已落盘的绑定表：taskId → { repoRoots, boundAt, lastConfirmedBy }
  // 渲染层用它在任务卡上画"未绑定"灰图标 vs "已绑定" 绿勾。
  // 绑定窗保存成功后调 refreshTaskBindings 拉一次最新数据。
  interface TaskBindingEntry {
    repoRoots: string[]
    boundAt: string
    lastConfirmedBy: string
  }
  const taskBindings = ref<Record<string, TaskBindingEntry>>({})
  const taskBindingsLoaded = ref(false)

  function isTaskBound(taskId: string): boolean {
    const e = taskBindings.value[taskId]
    return !!(e && e.repoRoots && e.repoRoots.length > 0)
  }

  async function refreshTaskBindings() {
    try {
      taskBindings.value = await invoke<Record<string, TaskBindingEntry>>('task_bindings_load')
      taskBindingsLoaded.value = true
    } catch (e) {
      // 没绑定表是正常情况（首次启动），保持空 map
      console.warn('[store] task_bindings_load 失败:', e)
      taskBindingsLoaded.value = true
    }
  }

  // ===== 今日复盘 =====

  const showReviewWindow = ref(false)
  /** 当天复盘是否已生成（避免 17:30 重复弹气泡） */
  const REVIEW_TRIGGERED_KEY = 'jarvis.reviewTriggered.v1'
  const loadReviewTriggeredOn = (): string => {
    try {
      return localStorage.getItem(REVIEW_TRIGGERED_KEY) ?? ''
    } catch {
      return ''
    }
  }
  const reviewTriggeredOn = ref<string>(loadReviewTriggeredOn())
  watch(reviewTriggeredOn, (v) => {
    try {
      localStorage.setItem(REVIEW_TRIGGERED_KEY, v)
    } catch {
      // ignore
    }
  })
  /** 当天是否已弹过"定今日计划"提示（避免重复弹） */
  const PLAN_PROMPTED_KEY = 'jarvis.planPrompted.v1'
  const loadPlanPromptedOn = (): string => {
    try {
      return localStorage.getItem(PLAN_PROMPTED_KEY) ?? ''
    } catch {
      return ''
    }
  }
  const todayPlanPromptedOn = ref<string>(loadPlanPromptedOn())
  watch(todayPlanPromptedOn, (v) => {
    try {
      localStorage.setItem(PLAN_PROMPTED_KEY, v)
    } catch {
      // ignore
    }
  })
  const reviewData = ref<DailyReviewData | null>(null)
  const reviewLoaded = ref(false)
  const reviewLoading = ref(false)
  const reviewLastError = ref<string | null>(null)

  function feedbackKey(taskId: string, sha: string) {
    return `${taskId}|${sha}`
  }

  function visibleCommitsForTask(taskId: string): CommitLink[] {
    const links = commitsByTask.value[taskId] ?? []
    return links.filter(l => commitFeedback.value[feedbackKey(taskId, l.sha)] !== 'rejected')
  }

  let toastId = 0

  function addToast(title: string, message: string) {
    const id = ++toastId
    toasts.value.push({ id, title, message })
    setTimeout(() => {
      toasts.value = toasts.value.filter(t => t.id !== id)
    }, 5000)
  }

  function removeToast(id: number) {
    toasts.value = toasts.value.filter(t => t.id !== id)
  }

  return {
    tasks,
    todayTasks,
    riskAnalysis,
    loading,
    showMenu,
    showTaskWindow,
    showAnalyzeWindow,
    toasts,
    addToast,
    removeToast,
    taskAlerts,
    alertsLoaded,
    alertsLastError,
    overdueTasks,
    todayAlertTasks,
    soonTasks,
    upcomingTasks,
    overdueCount,
    todayCount,
    soonCount,
    upcomingCount,
    stackedDays,
    alertLevel,
    showRiskWindow,
    showUpdateWindow,
    urgencyScored,
    myRemainingHours,
    hoursByDay,
    commitsByTask,
    commitsLoaded,
    commitsLastError,
    commitsRange,
    commitFeedback,
    visibleCommitsForTask,
    showReviewWindow,
    reviewTriggeredOn,
    todayPlanPromptedOn,
    reviewData,
    reviewLoaded,
    reviewLoading,
    reviewLastError,
    pendingBindTasks,
    showBindTaskWindow,
    enqueueBindTask,
    dequeueBindTask,
    taskBindings,
    taskBindingsLoaded,
    isTaskBound,
    refreshTaskBindings,
  }
})

export type CommitLink = {
  sha: string
  shortSha: string
  title: string
  authoredDate: string
  repoPath: string
  businessLine: string
  repoName: string
  matchType: 'exact' | 'soft'
  matchedKeywords?: string[]
}

export interface DailyReviewData {
  date: string
  range: { since: string; until: string; label: string }
  summary: {
    totalCommits: number
    businessLineCount: number
    tasksAdvancedCount: number
    orphanCommitCount: number
  }
  advancedTasks: Array<{
    taskId: string
    taskName: string
    status: string
    commitCount: number
    commits: CommitLink[]
    businessLine: string
    effort: number
    bindingConfidence: number
    bindingReason: string
    defaultWorkContent: string
    suggestedHours?: number
  }>
  byBusinessLine: Array<{
    businessLine: string
    commits: CommitLink[]
    tasks: Array<{ taskId: string; taskName: string }>
    effort: number
    suggestedHours?: number
  }>
  needsStatusUpdate: Array<{
    taskId: string
    taskName: string
    commitCount: number
    reason: string
  }>
  orphanCommits: Array<{
    businessLine: string
    commits: CommitLink[]
    effort?: number
    suggestedHours?: number
  }>
  totalHoursForEstimate: number
  /** 纯文本日报草稿，可直接复制粘贴 */
  plainText: string
  /** 禅道全部任务（不论是否有 commit 关联），供写工时搜索用 */
  allTasks: Array<{
    id: string
    name: string
    status: string
  }>
}
