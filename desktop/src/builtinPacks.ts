import { useConfigStore } from './stores/config'
import { petPackSave, type PetPack } from './api/petPack'
import { DA_FEI_YU_PACK_ID } from './petManifest'

// ============================================================================
// 内置宠物包：随应用分发、首启自动安装的官方包（当前唯一：大肥鱼）。
//
// 机制：素材 webm 作为构建资产打进安装包（import.meta.glob '?url'），avatar 主窗
// 启动时若 config.builtinPacksVersion 落后于 BUILTIN_PACK_VERSION，就把素材转
// base64 走 petPackSave 写入 ~/.jarvis/pets/（复用既有宠物包链路，零后端改动），
// 随后存版本号。用户删除后版本号已写、不会复活；升级素材集时递增版本号即重装。
//
// 版权署名常量（DA_FEI_YU_CREDIT / DA_FEI_YU_PACK_ID）在 petManifest.ts——
// 放那边是为了避免 petManifest → 本模块 → config store 的循环导入；出处与
// 许可口径见 petManifest.ts 顶部注释，本移植包按 CC BY-NC-SA（禁商用）分发，
// 未修改素材内容。
// ============================================================================

/** 内置包素材集版本：素材增删或替换时 +1，老用户重启即自动重装 */
export const BUILTIN_PACK_VERSION = 9

const assetUrls = import.meta.glob<string>(
  './assets/packs/dafeiyu/*.webp',
  { eager: true, query: '?url', import: 'default' },
)

function assetUrl(fileName: string): string {
  // fileName 已带 .webm 扩展名（pack 定义里的 file 原样传进来），不能再拼一次
  const url = assetUrls[`./assets/packs/dafeiyu/${fileName}`]
  if (!url) throw new Error(`内置大肥鱼素材缺失: ${fileName}`)
  return url
}

/** 大肥鱼宠物包定义（file 名对应 assets/packs/dafeiyu/ 下的 webm） */
export const DA_FEI_YU_PACK: PetPack = {
  id: DA_FEI_YU_PACK_ID,
  name: '大肥鱼',
  // 640×360 视频内角色高约 65%，×3 后约 131px——正好撑满 128px 命中盒，
  // 且画幅边界离角色更远（特效被画幅边切的观感更轻）
  scale: 3,
  facing: 'right',
  // 动画链：待机呼吸/东张西望/小动作/走路 按权重首尾相接，不再长呆滞
  chain: { idlePool: ['东张西望'], idleWeight: 55, actionsWeight: 35, walkWeight: 10 },
  clips: {
    idle: { file: '待机呼吸休闲.webp', type: 'webp' },
    walk: { file: '螃蟹走路.webp', type: 'webp' },
    held: { file: '被鼠标拖拽悬空反馈.webp', type: 'webp' },
    sleep: { file: '原地小憩沉眠.webp', type: 'webp' },
    // 工作状态档位（dsh-pet 六档口径；本应用映射 thinking→0/working→1/happy→4）
    workStatus: [
      { name: '思考冒泡', file: '工作状态-思考冒泡.webp', type: 'webp', durationMs: 10040 },
      { name: '忙碌点按', file: '工作状态-忙碌点按.webp', type: 'webp', durationMs: 10040 },
      { name: '清点归档', file: '工作状态-清点归档.webp', type: 'webp', durationMs: 10040 },
      { name: '原地踱步张望', file: '工作状态-原地踱步张望.webp', type: 'webp', durationMs: 10040 },
      { name: '雀跃庆祝', file: '工作状态-雀跃庆祝.webp', type: 'webp', durationMs: 10040 },
      { name: '垂头叹气冒汗', file: '工作状态-垂头叹气冒汗.webp', type: 'webp', durationMs: 4920 },
    ],
    // 点击回应池（点按宠物时随机抽 1，档内不连播）
    clicks: [
      { name: '开心跃动', file: '点击回应-开心跃动.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '害羞惊讶', file: '点击回应-害羞惊讶.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '傲娇生气', file: '点击回应-傲娇生气.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '挠痒咯咯笑', file: '点击回应-挠痒咯咯笑.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '元气挥手', file: '点击回应-元气挥手.webp', type: 'webp', durationMs: 10040, weight: 2 },
    ],
    actions: [
      // 转向 / 移动（原 turn / moves 池）。东张西望同时在 chain.idlePool 里
      // 轮换（hasClip 只认 actions，链条待机段靠它过门控）
      { name: '东张西望', file: '东张西望.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '原地漂浮踏步', file: '原地漂浮踏步.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '原地左转奔跑', file: '原地左转奔跑.webp', type: 'webp', durationMs: 10040, weight: 2 },
      // 小动作（原「小动作」分类精选）
      { name: '悠闲哼歌', file: '悠闲哼歌.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '超大伸懒腰', file: '超大伸懒腰.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '哈欠连天', file: '哈欠连天.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '女仆屈膝礼仪', file: '女仆屈膝礼仪.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '被吓一跳', file: '被吓一跳.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '写代码', file: '写代码.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '晨间刷牙', file: '晨间刷牙.webp', type: 'webp', durationMs: 10040, weight: 3 },
      // 玩耍（原「玩耍」分类精选）
      { name: '玩魔方', file: '原地专心玩魔方.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '鲸鱼吐泡泡', file: '鲸鱼吐泡泡特效.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '优雅女仆舞', file: '优雅女仆舞.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '可爱宅舞', file: '可爱宅舞.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '撸猫', file: '撸猫.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '拆礼物', file: '拆礼物.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '荡秋千', file: '荡秋千.webp', type: 'webp', durationMs: 10040, weight: 3 },
      { name: '骑木马', file: '骑木马.webp', type: 'webp', durationMs: 10040, weight: 3 },
      // 吃什么（原「吃什么」分类精选）
      { name: '吃白饭', file: '吃白饭.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '大口吃零食', file: '大口吃零食.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '吃Token', file: '吃Token.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '涮火锅', file: '涮火锅.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '吃糖葫芦', file: '吃糖葫芦.webp', type: 'webp', durationMs: 10040, weight: 2 },
      // 时节（原「时节」分类精选）
      { name: '堆雪人', file: '堆雪人.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '放烟花', file: '放烟花.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '舞狮头', file: '舞狮头.webp', type: 'webp', durationMs: 10040, weight: 2 },
      { name: '收红包', file: '收红包.webp', type: 'webp', durationMs: 10040, weight: 2 },
      // 文字（原「文字」分类，noMirror——朝左镜像时文字会反写，P1 事件引擎前先靠权重压低）
      { name: '是啊，吃什么', file: '是啊，吃什么.webp', type: 'webp', durationMs: 10040, weight: 1 },
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
  ...(DA_FEI_YU_PACK.clips.clicks ?? []).map(a => a.file),
  ...(DA_FEI_YU_PACK.clips.workStatus ?? []).map(a => a.file),
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
      scale: DA_FEI_YU_PACK.scale,
      chain: DA_FEI_YU_PACK.chain,
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
