// 插件基座共享类型与 invoke 封装（方案 C′）。
//
// 消费方：avatar 窗的插件引擎（usePluginHost.ts）与设置页「插件」管理
// （PluginsSection.vue）。类型只留一份避免漂移；对应 Rust plugins.rs /
// commands/plugins.rs，serde 全 camelCase，trigger/action 的 type 标签为
// snake_case（与 plugin.json 文档一致）。

import { invoke } from '@tauri-apps/api/core'

// ===== 清单结构（对应 Rust PluginManifest，camelCase 序列化） =====

export interface PluginButton {
  label: string
  /** 白名单命名动作：open_today_plan / open_manual_hours / open_review / open_tasks / dismiss */
  action: string
}

export type PluginAction =
  | {
      type: 'bubble'
      text: string
      emoji?: string
      state?: string
      /** 秒；0 = 持久气泡。缺省 12 */
      durationSec?: number
      buttons?: PluginButton[]
    }
  | {
      type: 'llm_line'
      /** pet_chatter 的 kind；"custom" = 用 instruction 当生成指令 */
      kind: string
      /** LLM 失败/未配置/关闭时的固定文案兜底（先弹，LLM 回来原位替换） */
      fallback: string
      instruction?: string
      emoji?: string
      state?: string
      durationSec?: number
    }
  | { type: 'notify'; title?: string; body: string }
  | { type: 'channel'; text: string }
  | { type: 'tool'; tool: string; args?: Record<string, unknown> }

export type PluginTrigger =
  | { type: 'cron'; expr: string; actions?: PluginAction[] }
  | {
      type: 'daily_window'
      startMin: number
      endMin: number
      /** 去重键：同插件多窗口时区分（缺省按下标） */
      key?: string
      actions?: PluginAction[]
    }
  | { type: 'event'; on: string; actions?: PluginAction[] }

export interface PluginManifest {
  id: string
  name: string
  description?: string
  version?: string
  author?: string
  triggers: PluginTrigger[]
  /** 插件级动作：触发器未自带 actions 时执行 */
  actions?: PluginAction[]
  /** 与上次实际执行的最小间隔（秒），防事件风暴 */
  minIntervalSec?: number
}

/** plugin_list 返回行（对应 Rust PluginInfo） */
export interface PluginInfo {
  id: string
  name: string
  description: string
  version: string
  source: 'builtin' | 'user'
  enabled: boolean
  /** 设置页展示用的人话摘要（Rust 拼好，如「窗口 07:30-09:00」） */
  triggerSummaries: string[]
  /** 校验失败原因；null = 有效 */
  invalidReason: string | null
  /** 全量清单（引擎执行用；invalid 时为 null） */
  manifest: PluginManifest | null
}

// ===== invoke 封装：参数名收口在这一处，防止调用方拼错 camelCase =====

export function pluginList(): Promise<PluginInfo[]> {
  return invoke<PluginInfo[]>('plugin_list')
}

export function pluginSetEnabled(id: string, enabled: boolean): Promise<void> {
  return invoke<void>('plugin_set_enabled', { id, enabled })
}

export function pluginReload(): Promise<void> {
  return invoke<void>('plugin_reload')
}

/** 打开（不存在则先创建并写入喝水示例）插件目录，返回目录路径 */
export function pluginsDirOpen(): Promise<string> {
  return invoke<string>('plugins_dir_open')
}

/** 广播应用事件（事件血管：task_overdue / commits_scanned / review_saved 等） */
export function appEvent(name: string, payload?: Record<string, unknown>): void {
  invoke('app_event', { name, payload: payload ?? null }).catch((e) =>
    console.error(`app_event ${name} 失败:`, e),
  )
}
