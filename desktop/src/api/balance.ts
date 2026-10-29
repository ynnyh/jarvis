import { invoke } from '@tauri-apps/api/core'

// DeepSeek 账户余额（桌宠余额分档动画的数据源）。
// 仅当前 LLM 凭证指向 DeepSeek 官方且配有 apiKey 时返回数据；
// 其余服务商 / 未配 key 返回 null（前端静默跳过）。

export interface DeepseekBalance {
  balance: number
  currency: string
}

export async function deepseekBalance(): Promise<DeepseekBalance | null> {
  return invoke<DeepseekBalance | null>('deepseek_balance')
}
