import { useConfigStore } from './stores/config'
import { petPackSave, type PetPack } from './api/petPack'

// ============================================================================
// 内置宠物包：随应用分发、首启自动安装的官方包（当前唯一：大肥鱼）。
//
// 机制：素材 webm 作为构建资产打进安装包（import.meta.glob '?url'），avatar 主窗
// 启动时若 config.builtinPacksVersion 落后于 BUILTIN_PACK_VERSION，就把素材转
// base64 走 petPackSave 写入 ~/.jarvis/pets/（复用既有宠物包链路，零后端改动），
// 随后存版本号。用户删除后版本号已写、不会复活；升级素材集时递增版本号即重装。
//
// 版权（不许删）：「大肥鱼」鲸鱼娘是 DeepSeek 的**社区二创形象**（非官方），
// 原案 @上善无形、女仆鲸鱼娘设计 @ZipZipPipe，CC BY-NC-SA 4.0；素材取自
// dsh-pet（github.com/PC2005-cloud/dsh-pet，v0.3.8，素材「允许开源使用、禁止
// 商用」）。本移植包按同口径（CC BY-NC-SA、禁止商用）随应用分发，仅做子集
// 挑选与格式直迁、未修改素材内容。
// ============================================================================

/** 内置包素材集版本：素材增删或替换时 +1，老用户重启即自动重装 */
export const BUILTIN_PACK_VERSION = 1

export const DA_FEI_YU_PACK_ID = 'dafeiyu'

/** 大肥鱼署名文案（设置列表描述用；与上面版权注释同口径） */
export const DA_FEI_YU_CREDIT =
  'DeepSeek 二创「大肥鱼」鲸鱼娘（非官方）· 形象 CC BY-NC-SA 4.0（@上善无形 原案 / @ZipZipPipe 设计）· 素材自 dsh-pet · 禁止商用'

const assetUrls = import.meta.glob<string>(
  './assets/packs/dafeiyu/*.webm',
  { eager: true, query: '?url', import: 'default' },
)

function assetUrl(fileName: string): string {
  const url = assetUrls[`./assets/packs/dafeiyu/${fileName}.webm`]
  if (!url) throw new Error(`内置大肥鱼素材缺失: ${fileName}`)
  return url
}

/** 大肥鱼宠物包定义（file 名对应 assets/packs/dafeiyu/ 下的 webm） */
export const DA_FEI_YU_PACK: PetPack = {
  id: DA_FEI_YU_PACK_ID,
  name: '大肥鱼',
  // 640×360 视频内角色高约 65%，×2 后在 128px 档约 94px 高（真机可再调）
  scale: 2,
  facing: 'right',
  clips: {
    idle: { file: '待机呼吸休闲.webm', type: 'webm' },
    walk: { file: '螃蟹走路.webm', type: 'webm' },
    held: { file: '被鼠标拖拽悬空反馈.webm', type: 'webm' },
    sleep: { file: '原地小憩沉眠.webm', type: 'webm' },
    actions: [
      // 点击回应（原 clicks 池）
      { name: '开心跃动', file: '点击回应-开心跃动.webm', type: 'webm', weight: 2 },
      { name: '害羞惊讶', file: '点击回应-害羞惊讶.webm', type: 'webm', weight: 2 },
      { name: '傲娇生气', file: '点击回应-傲娇生气.webm', type: 'webm', weight: 2 },
      { name: '挠痒咯咯笑', file: '点击回应-挠痒咯咯笑.webm', type: 'webm', weight: 2 },
      { name: '元气挥手', file: '点击回应-元气挥手.webm', type: 'webm', weight: 2 },
      // 转向 / 移动（原 turn / moves 池）
      { name: '东张西望', file: '东张西望.webm', type: 'webm', weight: 2 },
      { name: '原地漂浮踏步', file: '原地漂浮踏步.webm', type: 'webm', weight: 2 },
      { name: '原地左转奔跑', file: '原地左转奔跑.webm', type: 'webm', weight: 2 },
      // 小动作（原「小动作」分类精选）
      { name: '悠闲哼歌', file: '悠闲哼歌.webm', type: 'webm', weight: 3 },
      { name: '超大伸懒腰', file: '超大伸懒腰.webm', type: 'webm', weight: 3 },
      { name: '哈欠连天', file: '哈欠连天.webm', type: 'webm', weight: 3 },
      { name: '女仆屈膝礼仪', file: '女仆屈膝礼仪.webm', type: 'webm', weight: 3 },
      { name: '被吓一跳', file: '被吓一跳.webm', type: 'webm', weight: 3 },
      { name: '写代码', file: '写代码.webm', type: 'webm', weight: 3 },
      { name: '晨间刷牙', file: '晨间刷牙.webm', type: 'webm', weight: 3 },
      // 玩耍（原「玩耍」分类精选）
      { name: '玩魔方', file: '原地专心玩魔方.webm', type: 'webm', weight: 3 },
      { name: '鲸鱼吐泡泡', file: '鲸鱼吐泡泡特效.webm', type: 'webm', weight: 3 },
      { name: '优雅女仆舞', file: '优雅女仆舞.webm', type: 'webm', weight: 3 },
      { name: '可爱宅舞', file: '可爱宅舞.webm', type: 'webm', weight: 3 },
      { name: '撸猫', file: '撸猫.webm', type: 'webm', weight: 3 },
      { name: '拆礼物', file: '拆礼物.webm', type: 'webm', weight: 3 },
      { name: '荡秋千', file: '荡秋千.webm', type: 'webm', weight: 3 },
      { name: '骑木马', file: '骑木马.webm', type: 'webm', weight: 3 },
      // 吃什么（原「吃什么」分类精选）
      { name: '吃白饭', file: '吃白饭.webm', type: 'webm', weight: 2 },
      { name: '大口吃零食', file: '大口吃零食.webm', type: 'webm', weight: 2 },
      { name: '吃Token', file: '吃Token.webm', type: 'webm', weight: 2 },
      { name: '涮火锅', file: '涮火锅.webm', type: 'webm', weight: 2 },
      { name: '吃糖葫芦', file: '吃糖葫芦.webm', type: 'webm', weight: 2 },
      // 时节（原「时节」分类精选）
      { name: '堆雪人', file: '堆雪人.webm', type: 'webm', weight: 2 },
      { name: '放烟花', file: '放烟花.webm', type: 'webm', weight: 2 },
      { name: '舞狮头', file: '舞狮头.webm', type: 'webm', weight: 2 },
      { name: '收红包', file: '收红包.webm', type: 'webm', weight: 2 },
      // 文字（原「文字」分类，noMirror——朝左镜像时文字会反写，P1 事件引擎前先靠权重压低）
      { name: '是啊，吃什么', file: '是啊，吃什么.webm', type: 'webm', weight: 1 },
    ],
  },
}

/** 全部素材文件名（安装时逐一转 base64） */
const DA_FEI_YU_FILES: string[] = [
  DA_FEI_YU_PACK.clips.idle.file,
  DA_FEI_YU_PACK.clips.walk?.file,
  DA_FEI_YU_PACK.clips.held?.file,
  DA_FEI_YU_PACK.clips.sleep?.file,
  ...(DA_FEI_YU_PACK.clips.actions ?? []).map(a => a.file),
].filter((f): f is string => !!f)

function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = () => reject(new Error('内置包素材读取失败'))
    reader.readAsDataURL(blob)
  })
}

let ensuring: Promise<boolean> | null = null

/**
 * 首启安装内置包。返回 true 表示本次执行了安装（调用方需要 reloadPackPets）。
 * 已是最新版本 / 正在安装中 → 幂等返回。任一素材或写入失败：不落版本号，
 * 下次启动自动重试（部分写入由 petPackSave 覆盖写保证最终一致）。
 */
export async function ensureBuiltinPacks(): Promise<boolean> {
  if (ensuring) return ensuring
  ensuring = (async () => {
    const store = useConfigStore()
    if ((store.config.builtinPacksVersion ?? 0) >= BUILTIN_PACK_VERSION) return false
    const files = await Promise.all(
      DA_FEI_YU_FILES.map(async (name) => {
        const res = await fetch(assetUrl(name))
        if (!res.ok) throw new Error(`内置包素材加载失败: ${name}`)
        const dataUrl = await blobToDataUrl(await res.blob())
        return { name, dataBase64: dataUrl.slice(dataUrl.indexOf(',') + 1) }
      }),
    )
    await petPackSave({
      id: DA_FEI_YU_PACK.id,
      name: DA_FEI_YU_PACK.name,
      facing: DA_FEI_YU_PACK.facing,
      clips: DA_FEI_YU_PACK.clips,
      files,
    })
    store.config.builtinPacksVersion = BUILTIN_PACK_VERSION
    await store.save()
    return true
  })()
  try {
    return await ensuring
  } finally {
    ensuring = null
  }
}
