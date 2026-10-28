import { invoke } from '@tauri-apps/api/core'

// 宠物包 v2（多动作素材包）的前后端契约层。
// 后端实现在 src-tauri/src/commands/pet_pack.rs，素材落盘为真实文件目录
// ~/.jarvis/pets/<packId>/，前端按 clip 惰性经 pet_pack_read_file 换 data URL。
// invoke 参数 key 一律 camelCase（type-safety spec：snake_case 会静默失效）。

export type PetPackClipType = 'gif' | 'webm' | 'webp' | 'spritesheet'
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
  /** 一次性片段时长毫秒（webp/gif 走 img 路径没有 ended 事件，按此收尾）；
   *  缺省用引擎常量兜底。loop 片段不需要 */
  durationMs?: number
}

/** 小动作片段：name 进点播菜单，weight 是随机权重（缺省 1）。
 *  category 声明点播菜单的分类（如 移动/小动作/玩耍/吃什么/时节），
 *  任一动作带 category → 「来一个」区块变为 分类→动作 两级点播；
 *  全部不带 → 维持洗牌 6 个的平铺现状。 */
export interface PetPackAction extends PetPackClip {
  name: string
  weight?: number
  category?: string
}

/** 动画链（v3，对齐 dsh-pet）：一次性动作播完后按权重接续下一段，待机/小动作
 *  首尾相接不再长呆滞。idlePool 是进入待机轮换的一次性动画名（须在 actions 里
 *  声明）；三权重缺省 60/30/10，和为 0 时链不运转。缺省整个字段 = 现状行为。 */
export interface PetPackChain {
  idlePool?: string[]
  idleWeight?: number
  actionsWeight?: number
  /** 链上发起一次漫游走路的权重（走路仍由 roamer 规划路径与安全边界） */
  walkWeight?: number
}

export interface PetPackClips {
  /** 唯一必填片段 */
  idle: PetPackClip
  walk?: PetPackClip
  held?: PetPackClip
  sleep?: PetPackClip
  actions?: PetPackAction[]
  /** 点击回应池：点按宠物时随机抽 1 播（档内不连播同一段）；缺省无回应动画 */
  clicks?: PetPackAction[]
  /** 工作状态档位动画（对齐 dsh-pet 六档，索引即档位）：
   *  0思考 1工作 2整理 3等待 4成功(播一遍) 5出错(播一遍)。
   *  本应用状态机目前映射 thinking→0 / working→1 / happy→4，其余状态不接管。
   *  缺省无该字段 = 维持 stateMap 现状（色变表达）。 */
  workStatus?: PetPackAction[]
}

export interface PetPack {
  id: string
  name: string
  /** 整体缩放倍率，缺省 1 */
  scale?: number
  facing: PetPackFacing
  clips: PetPackClips
  /** 动画链配置，缺省 = 现状行为（roamer 低频决策） */
  chain?: PetPackChain
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
  /** 动画链配置（v3），缺省不写 */
  chain?: PetPackChain
}): Promise<void> {
  return invoke('pet_pack_save', {
    id: pack.id,
    name: pack.name,
    facing: pack.facing,
    clips: pack.clips,
    files: pack.files,
    scale: pack.scale,
    chain: pack.chain,
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
