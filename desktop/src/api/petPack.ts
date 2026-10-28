import { invoke } from '@tauri-apps/api/core'

// 宠物包 v2（多动作素材包）的前后端契约层。
// 后端实现在 src-tauri/src/commands/pet_pack.rs，素材落盘为真实文件目录
// ~/.jarvis/pets/<packId>/，前端按 clip 惰性经 pet_pack_read_file 换 data URL。
// invoke 参数 key 一律 camelCase（type-safety spec：snake_case 会静默失效）。

export type PetPackClipType = 'gif' | 'webm' | 'spritesheet'
export type PetPackFacing = 'left' | 'right'

/** 单个动画片段：gif/webm 直渲；spritesheet 额外带帧参数走 CSS steps 动画 */
export interface PetPackClip {
  file: string
  type: PetPackClipType
  // —— spritesheet 专用（CSS steps 关键帧用） ——
  frameW?: number
  frameH?: number
  fps?: number
  frames?: number
  columns?: number
}

/** 小动作片段：name 进点播菜单，weight 是随机权重（缺省 1） */
export interface PetPackAction extends PetPackClip {
  name: string
  weight?: number
}

export interface PetPackClips {
  /** 唯一必填片段 */
  idle: PetPackClip
  walk?: PetPackClip
  held?: PetPackClip
  sleep?: PetPackClip
  actions?: PetPackAction[]
}

export interface PetPack {
  id: string
  name: string
  /** 整体缩放倍率，缺省 1 */
  scale?: number
  facing: PetPackFacing
  clips: PetPackClips
}

/** 上传素材：原文件名（basename）+ base64 内容 */
export interface PetPackFileIn {
  name: string
  dataBase64: string
}

export async function petPackList(): Promise<PetPack[]> {
  return invoke<PetPack[]>('pet_pack_list')
}

export async function petPackSave(pack: {
  id: string
  name: string
  facing: PetPackFacing
  clips: PetPackClips
  files: PetPackFileIn[]
  /** 整体缩放倍率，缺省 1.0 */
  scale?: number
}): Promise<void> {
  return invoke('pet_pack_save', {
    id: pack.id,
    name: pack.name,
    facing: pack.facing,
    clips: pack.clips,
    files: pack.files,
    scale: pack.scale,
  })
}

export async function petPackDelete(id: string): Promise<void> {
  return invoke('pet_pack_delete', { id })
}

/** 读包内素材 → data URL（mime 按扩展名，后端定） */
export async function petPackReadFile(packId: string, file: string): Promise<string> {
  return invoke<string>('pet_pack_read_file', { packId, file })
}

/** packId 预净化（与后端 sanitize_pack_id 同规则）：中文文件夹名剥成可用 ID */
export function sanitizePackIdLocal(name: string): string {
  const cleaned = name.replace(/[^a-zA-Z0-9_-]/g, '')
  // 纯中文文件夹名剥完为空 → 用时间戳兜底，保证可保存
  return cleaned || `pack-${Date.now().toString(36)}`
}
