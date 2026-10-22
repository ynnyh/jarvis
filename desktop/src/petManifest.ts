// 宠物形象注册表。每个宠物 = 一个 Lottie JSON + 一组元信息。
//
// 内置宠物：把 Lottie .json 文件放到 desktop/src/assets/pets/，在 PETS 数组里加一项。
// 自定义宠物：通过设置界面上传 Lottie/图片/GIF，存储在 ~/.jarvis/custom-pets/。
//
// 暂用 eager import：全部内置宠物打包进 JS bundle。
// 如果以后内置太多影响加载速度再改 lazy import。

import { shallowRef } from 'vue'
import roboData from './assets/pets/robo.json'
import catMoonData from './assets/pets/cat-moon.json'
import astroLaptopData from './assets/pets/astro-laptop.json'
import astroHeroData from './assets/pets/astro-hero.json'
import happySpacemanData from './assets/pets/Happy Spaceman.json'
import catCryingData from './assets/pets/Cat Crying emojiSticker animation.json'
import cuteTigerData from './assets/pets/Cute Tiger.json'
import slothMeditateData from './assets/pets/Sloth meditate.json'
import cowDrinkMilkData from './assets/pets/Cow Drink Milk.json'
import dancingLlamaData from './assets/pets/Dancing llama.json'
import { customPetList, type CustomPet, type ImageAnimation } from './api/customPet'
import { petPackList, petPackReadFile, type PetPack } from './api/petPack'

export type PetCategory = 'mecha' | 'pet' | 'character' | 'custom' | 'pack'
export interface PetRenderConfig {
  scale?: number
  offsetX?: number
  offsetY?: number
}

export interface PetInfo {
  id: string
  name: string
  category: PetCategory
  /** 一句话描述，给设置 UI 提示文案用 */
  description: string
  /** Lottie 动画 JSON 数据，或图片/GIF 的 Base64 数据；pack 类型无内联 data（素材按 clip 惰性加载） */
  data: unknown
  render?: PetRenderConfig
  /** 自定义宠物类型：lottie | image | gif；宠物包为 pack */
  petType?: 'lottie' | 'image' | 'gif' | 'pack'
  /** 仅图片类型：动画效果 */
  imageAnimation?: ImageAnimation
  /** 仅 pack 类型：完整包清单（clips/scale/facing） */
  pack?: PetPack
}

export const PETS: PetInfo[] = [
  {
    id: 'robo',
    name: '小机器人',
    category: 'mecha',
    description: '机甲风默认形象',
    data: roboData,
  },
  {
    id: 'cat-moon',
    name: '月球钓鱼猫',
    category: 'pet',
    description: '在月亮上钓鱼的小猫',
    data: catMoonData,
  },
  {
    id: 'astro-laptop',
    name: '敲键盘宇航员',
    category: 'character',
    description: '抱着笔记本敲代码的小宇航员',
    data: astroLaptopData,
  },
  {
    id: 'astro-hero',
    name: '飞行宇航员',
    category: 'character',
    description: '披风飞行的超级宇航员',
    data: astroHeroData,
  },
  {
    id: 'happy-spaceman',
    name: '开心太空人',
    category: 'character',
    description: '快乐漂浮的太空人',
    data: happySpacemanData,
  },
  {
    id: 'cat-crying',
    name: '哭泣小猫',
    category: 'pet',
    description: '委屈巴巴的哭泣猫咪',
    data: catCryingData,
  },
  {
    id: 'cute-tiger',
    name: '可爱小虎',
    category: 'pet',
    description: '呆萌可爱的小老虎',
    data: cuteTigerData,
  },
  {
    id: 'sloth-meditate',
    name: '冥想树懒',
    category: 'pet',
    description: '闭目冥想的树懒',
    data: slothMeditateData,
  },
  {
    id: 'cow-drink-milk',
    name: '喝奶小牛',
    category: 'pet',
    description: '憨态可掬喝牛奶的小牛',
    data: cowDrinkMilkData,
    render: { scale: 3, offsetY: 1 },
  },
  {
    id: 'dancing-llama',
    name: '跳舞羊驼',
    category: 'pet',
    description: '魔性舞蹈的羊驼',
    data: dancingLlamaData,
  },
]

export const PET_CATEGORY_LABELS: Record<PetCategory, string> = {
  mecha: '机甲',
  pet: '宠物',
  character: '人物',
  custom: '自定义',
  pack: '宠物包',
}

export function isCustomPetId(id: string): boolean {
  return id.startsWith('custom-')
}

export function isPackPetId(id: string): boolean {
  return id.startsWith('pack-')
}

// 自定义宠物缓存（响应式）。
// avatar 主窗口与 settings 窗口是两个独立 webview，各自持有一份模块缓存。
// settings 窗口上传/切换自定义宠物后，主窗口靠 config-changed 同步刷新。
// 用 shallowRef 让 PetAvatar 的 computed 能追踪缓存变化自动重渲，又不会把
// Lottie/图片的大 data 对象深度代理（性能 & # 私有字段问题）。
const customPetsCache = shallowRef<PetInfo[]>([])

/** 加载自定义宠物列表（从后端读取，覆盖缓存） */
export async function loadCustomPets(): Promise<PetInfo[]> {
  try {
    const remotePets = await customPetList()
    customPetsCache.value = remotePets.map(toPetInfo)
    return customPetsCache.value
  } catch (e) {
    console.error('加载自定义宠物失败:', e)
    return customPetsCache.value
  }
}

/** 获取缓存的自定义宠物列表（同步，需先调用 loadCustomPets） */
export function getCustomPets(): PetInfo[] {
  return customPetsCache.value
}

function toPetInfo(pet: CustomPet): PetInfo {
  return {
    id: pet.id,
    name: pet.name,
    category: 'custom',
    description: pet.description,
    data: pet.data,
    petType: pet.type,
    imageAnimation: pet.animation,
  }
}

// ===== 宠物包 v2（多动作素材包）=====
// 与自定义宠物同款缓存模式：每个 webview 一份模块缓存，落盘后靠
// pet-packs-changed / config-changed 广播刷新。素材不进缓存 —— data URL
// 按 clip 惰性加载（getPackClipSrc），避免整包 60MB 一次性进内存。

const packPetsCache = shallowRef<PetInfo[]>([])

/** 从包清单推导设置列表里的一句话描述 */
function packDescription(pack: PetPack): string {
  const parts: string[] = []
  if (pack.clips.walk) parts.push('可漫步')
  if (pack.clips.held) parts.push('可拎起')
  if (pack.clips.sleep) parts.push('可睡觉')
  const actionCount = pack.clips.actions?.length ?? 0
  if (actionCount > 0) parts.push(`${actionCount} 个小动作`)
  return parts.length > 0 ? parts.join(' · ') : '单动作'
}

function toPackPetInfo(pack: PetPack): PetInfo {
  return {
    // pack- 前缀与内置/自定义 id 空间隔离，杜绝用户包 id 撞内置名
    id: `pack-${pack.id}`,
    name: pack.name,
    category: 'pack',
    description: packDescription(pack),
    data: null,
    petType: 'pack',
    pack,
  }
}

/** 加载宠物包列表（从后端读取，覆盖缓存） */
export async function loadPackPets(): Promise<PetInfo[]> {
  try {
    const packs = await petPackList()
    packPetsCache.value = packs.map(toPackPetInfo)
  } catch (e) {
    console.error('加载宠物包失败:', e)
  }
  return packPetsCache.value
}

/** 获取缓存的宠物包列表（同步，需先调用 loadPackPets） */
export function getPackPets(): PetInfo[] {
  return packPetsCache.value
}

/** pack 宠物是否拥有某片段（idle 恒真；walk/held/sleep 查对应槽；其余查 actions 名）。
 *  纯函数读 shallowRef 缓存 —— E2 漫游引擎在 computed 里调用即可获得响应式。 */
export function packHasClip(petId: string, name: string): boolean {
  if (!isPackPetId(petId)) return false
  const pack = getPackPets().find(p => p.id === petId)?.pack
  if (!pack) return false
  if (name === 'idle') return !!pack.clips.idle
  if (name === 'walk' || name === 'held' || name === 'sleep') return !!pack.clips[name]
  return (pack.clips.actions ?? []).some(a => a.name === name)
}

// 素材 data URL 缓存（模块级，跨组件重挂载复用）；pending 表去重并发请求
const packClipSrcCache = new Map<string, string>()
const packClipPending = new Map<string, Promise<string | null>>()

/** 惰性加载包内素材 → data URL（带缓存）。失败返回 null，调用方走降级。 */
export async function getPackClipSrc(packId: string, file: string): Promise<string | null> {
  const key = `${packId}/${file}`
  const hit = packClipSrcCache.get(key)
  if (hit) return hit
  const pending = packClipPending.get(key)
  if (pending) return pending
  const task = petPackReadFile(packId, file)
    .then((url) => {
      packClipSrcCache.set(key, url)
      return url
    })
    .catch((e) => {
      console.error('读取宠物包素材失败:', e)
      return null
    })
    .finally(() => {
      packClipPending.delete(key)
    })
  packClipPending.set(key, task)
  return task
}

/** 清空素材缓存（包重新导入/删除后调用，防止旧 data URL 残留） */
export function clearPackClipCache(packId?: string): void {
  if (!packId) {
    packClipSrcCache.clear()
    return
  }
  const prefix = `${packId}/`
  for (const key of packClipSrcCache.keys()) {
    if (key.startsWith(prefix)) packClipSrcCache.delete(key)
  }
}

/** 获取所有宠物（内置 + 自定义 + 宠物包），pack 与旧类型并存于同一选择列表 */
export function getAllPets(): PetInfo[] {
  return [...PETS, ...getCustomPets(), ...getPackPets()]
}

export function getPetById(id: string): PetInfo {
  if (isCustomPetId(id)) {
    const custom = getCustomPets().find(p => p.id === id)
    if (custom) return custom
  }
  if (isPackPetId(id)) {
    const pack = getPackPets().find(p => p.id === id)
    if (pack) return pack
  }
  return PETS.find(p => p.id === id) ?? PETS[0]
}
