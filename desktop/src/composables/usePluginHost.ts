/**
 * 声明式插件引擎（方案 C′3，avatar 窗）。
 *
 * 消费 plugin.json 清单：触发器（cron / 每日时间窗 / 应用事件）→ 顺序执行动作
 * （bubble / llm_line / notify / channel / tool）。纯解释执行，不可执行任意代码。
 *
 * 设施复用（照抄既有提醒 composables 的模式）：
 *   - cron：croner `job.match(now)` + 分钟键去重（useScheduledReminders 同款）
 *   - daily_window：dailyFired 每日一次去重（useTimeGreetings 同款）
 *   - 事件：listen('app-event')，Rust worklog/deploy 与前端三处发射点供血
 *   - llm_line：复用 B 的 petChatter 链路（含 petProactiveChat 总开关、固定文案
 *     先行、LLM 原位替换）——greeting 迁移后 LLM 升级能力不回退
 *   - tool：走 plugin_tool_execute（Rust 侧安全门禁，插件只能调安全级工具，
 *     失败静默 log，不弹确认框）
 *   - minIntervalSec：内存 map 记 lastFired，防事件风暴
 *
 * 容错：单插件/单动作失败只 log，不影响其他插件（invalid 清单在 Rust 侧已标
 * 记 invalidReason、manifest 为 null，引擎天然跳过）。
 */

import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { sendNotification } from '@tauri-apps/plugin-notification'
import { Cron } from 'croner'
import { useConfigStore } from '../stores/config'
import { useSharedTick } from './useSharedTick'
import { hasFired, markFired } from './dailyFired'
import { pluginList } from '../api/plugins'
import type {
  PluginInfo,
  PluginManifest,
  PluginTrigger,
  PluginAction,
  PluginButton,
} from '../api/plugins'

/** bubble.state 允许的 8 态（与 useWindowOrchestration 的 JarvisState 结构一致） */
export type PluginBubbleState =
  | 'idle' | 'thinking' | 'working' | 'warning' | 'happy' | 'morning' | 'coffee' | 'late'

const PLUGIN_STATES = new Set<string>([
  'idle', 'thinking', 'working', 'warning', 'happy', 'morning', 'coffee', 'late',
])

/** 按钮命名动作 → avatar 窗既有菜单函数的映射类型 */
export interface PluginMenuActions {
  openTodayPlan: () => void
  openManualHours: () => void
  openReview: () => void
  openTasks: () => void
  dismiss: () => void
}

export interface PluginHostOptions {
  showAlert: (
    text: string,
    emoji: string,
    state: PluginBubbleState,
    duration: number,
    actions?: Array<{ label: string; action: () => void | Promise<void> }>,
  ) => void
  /** B2 的搭话链路：固定文案先行，LLM（≤5s）回来原位替换；kind="custom" 走 context.instruction */
  petChatter: (
    kind: string,
    fallback: { text: string; emoji: string; state: PluginBubbleState; duration: number },
    context?: Record<string, unknown>,
  ) => void
  menu: PluginMenuActions
}

/** app-event 信封（对应 Rust AppEventEnvelope，camelCase） */
interface AppEventEnvelope {
  name: string
  payload?: Record<string, unknown>
}

export function usePluginHost(options: PluginHostOptions) {
  const configStore = useConfigStore()
  const plugins = ref<PluginInfo[]>([])
  // cron 分钟键去重（值含完整分钟时间戳，过期即清）
  const firedKeys = new Set<string>()
  // minIntervalSec 的 lastFired 记录（内存即可，重启清零无妨）
  const lastFiredMs = new Map<string, number>()
  let unlistenPluginsChanged: UnlistenFn | null = null
  let unlistenAppEvent: UnlistenFn | null = null

  /** 引擎实际执行的插件：enabled 且清单有效（invalid 在 Rust 已滤成 manifest=null） */
  const activePlugins = computed(() =>
    plugins.value.filter(p => p.enabled && p.manifest),
  )

  async function reload() {
    try {
      plugins.value = await pluginList()
    } catch (e) {
      console.error('plugin_list 失败，插件本回合停用:', e)
    }
  }

  // 配置变更（他窗开关插件 / 本窗 plugin_set_enabled 广播）→ 重新拉列表。
  // 统一走重拉而非本地过滤：enabled 由 Rust 按 disabledPlugins 现算，一处逻辑。
  watch(
    () => configStore.config.disabledPlugins,
    () => { void reload() },
  )

  // ===== 模板变量：{userTitle} / {assistantName} / {payload.<字段>}（缺字段→空串） =====

  function interpolate(text: string, payload?: Record<string, unknown>): string {
    return text
      .replaceAll('{userTitle}', configStore.config.userTitle)
      .replaceAll('{assistantName}', configStore.config.assistantName)
      .replace(/\{payload\.([\w.-]+)\}/g, (_m, field: string) => {
        const v = payload?.[field]
        return v === undefined || v === null ? '' : String(v)
      })
  }

  function normState(s: string | undefined): PluginBubbleState {
    return s && PLUGIN_STATES.has(s) ? (s as PluginBubbleState) : 'idle'
  }

  // ===== 动作执行 =====

  function mapButtons(buttons: PluginButton[] | undefined) {
    const TABLE: Record<string, () => void> = {
      open_today_plan: options.menu.openTodayPlan,
      open_manual_hours: options.menu.openManualHours,
      open_review: options.menu.openReview,
      open_tasks: options.menu.openTasks,
      dismiss: options.menu.dismiss,
    }
    return (buttons ?? [])
      .map(b => ({ label: b.label, action: TABLE[b.action] }))
      .filter((b): b is { label: string; action: () => void } => typeof b.action === 'function')
  }

  async function runAction(p: PluginInfo, act: PluginAction, payload?: Record<string, unknown>) {
    switch (act.type) {
      case 'bubble':
        // durationSec=0 → 持久气泡（showAlert 语义：duration 0 不自动消失）
        options.showAlert(
          interpolate(act.text, payload),
          act.emoji || '🔔',
          normState(act.state),
          (act.durationSec ?? 12) * 1000,
          mapButtons(act.buttons),
        )
        break
      case 'llm_line':
        // petChatter 内部：showAlert(fallback) 先行 + petProactiveChat 开关门控 +
        // LLM 未配置/失败/超时静默兜底（返回 None 时固定文案已在场）
        options.petChatter(
          act.kind,
          {
            text: interpolate(act.fallback, payload),
            emoji: act.emoji || '💬',
            state: normState(act.state),
            duration: (act.durationSec ?? 15) * 1000,
          },
          act.kind === 'custom' ? { instruction: interpolate(act.instruction ?? '', payload) } : {},
        )
        break
      case 'notify':
        try {
          sendNotification({
            title: interpolate(act.title ?? '', payload) || configStore.config.assistantName,
            body: interpolate(act.body, payload),
          })
        } catch (e) {
          console.warn(`[plugin] ${p.id} notify 失败:`, e)
        }
        break
      case 'channel':
        try {
          await invoke('channels_notify', { text: interpolate(act.text, payload) })
        } catch (e) {
          console.warn(`[plugin] ${p.id} channel 推送失败:`, e)
        }
        break
      case 'tool':
        // 安全门禁在 Rust（plugin_tool_execute）：写操作工具/confirm 级 MCP 直接拒。
        // 插件场景没有确认 UI，失败只 log 不弹框——文档口径「插件只能调安全级工具」。
        try {
          const r = await invoke<{ success: boolean; error?: string }>('plugin_tool_execute', {
            name: act.tool,
            input: act.args ?? {},
          })
          if (!r?.success) {
            console.warn(`[plugin] ${p.id} tool ${act.tool} 未执行:`, r?.error)
          }
        } catch (e) {
          console.warn(`[plugin] ${p.id} tool ${act.tool} 调用失败:`, e)
        }
        break
    }
  }

  /** 触发器命中 → 顺序执行动作（触发器自带 actions 优先，否则插件级） */
  async function firePlugin(
    p: PluginInfo,
    trigger: PluginTrigger,
    payload?: Record<string, unknown>,
  ) {
    const manifest = p.manifest as PluginManifest
    // minIntervalSec：距上次实际执行不足则整插件跳过（防事件触发风暴）
    const minMs = (manifest.minIntervalSec ?? 0) * 1000
    if (minMs > 0) {
      const now = Date.now()
      const last = lastFiredMs.get(p.id) ?? 0
      if (now - last < minMs) return
      lastFiredMs.set(p.id, now)
    }
    const actions = (trigger.actions && trigger.actions.length > 0)
      ? trigger.actions
      : (manifest.actions ?? [])
    for (const act of actions) {
      try {
        await runAction(p, act, payload)
      } catch (e) {
        console.warn(`[plugin] ${p.id} 动作 ${act.type} 执行失败:`, e)
      }
    }
  }

  // ===== 触发器：cron + daily_window（每分钟核对）=====

  function minuteKey(now: Date): string {
    const pad = (n: number) => String(n).padStart(2, '0')
    return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`
  }

  function tick() {
    if (!configStore.loaded) return
    const now = new Date()
    const dateKey = minuteKey(now)
    // 过期分钟键清理（照抄 useScheduledReminders：value 含分钟戳，比对后删）
    for (const k of firedKeys) {
      if (!k.endsWith(dateKey)) firedKeys.delete(k)
    }
    for (const p of activePlugins.value) {
      const manifest = p.manifest as PluginManifest
      manifest.triggers.forEach((t, ti) => {
        if (t.type === 'cron') {
          const key = `plugin.${p.id}.${ti}.${dateKey}`
          if (firedKeys.has(key)) return
          try {
            // croner 解析失败（坏表达式）跳过并 log，与 useScheduledReminders 同策略
            if (!new Cron(t.expr).match(now)) return
          } catch {
            console.warn(`[plugin] ${p.id} cron 表达式无效: ${t.expr}`)
            return
          }
          firedKeys.add(key)
        } else if (t.type === 'daily_window') {
          const minutes = now.getHours() * 60 + now.getMinutes()
          if (minutes < t.startMin || minutes >= t.endMin) return
          // 每自然日一次（localStorage 键按日期自然失效），category=plugin 防撞车
          const dkey = `${p.id}.${t.key ?? ti}`
          if (hasFired('plugin', dkey)) return
          markFired('plugin', dkey)
        } else {
          return // event 触发器只在 app-event 回调里跑
        }
        void firePlugin(p, t)
      })
    }
  }

  // ===== 触发器：应用事件 =====

  function onAppEvent(env: AppEventEnvelope) {
    if (!configStore.loaded || !env?.name) return
    for (const p of activePlugins.value) {
      const manifest = p.manifest as PluginManifest
      for (const t of manifest.triggers) {
        // 事件触发不去重：幂等性由插件自己保证（PRD C′1 文档口径）
        if (t.type === 'event' && t.on === env.name) {
          void firePlugin(p, t, env.payload)
        }
      }
    }
  }

  useSharedTick(tick)

  onMounted(async () => {
    await reload()
    unlistenPluginsChanged = await listen('plugins-changed', () => { void reload() })
    unlistenAppEvent = await listen<AppEventEnvelope>('app-event', (e) => onAppEvent(e.payload))
  })

  onUnmounted(() => {
    // 前端监听 onUnmounted 反注册（useWindowOrchestration 同款纪律）
    unlistenPluginsChanged?.()
    unlistenAppEvent?.()
    unlistenPluginsChanged = null
    unlistenAppEvent = null
  })
}
