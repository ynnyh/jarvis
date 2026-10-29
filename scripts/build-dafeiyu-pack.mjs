// 内置「大肥鱼」宠物包素材同步脚本（P0）。
//
// 来源：dsh-pet（github.com/PC2005-cloud/dsh-pet）npm 包 assets/webm/，
// VP9-alpha 透明 webm。版权：形象 CC BY-NC-SA 4.0（@上善无形 原案 /
// @ZipZipPipe 设计），素材允许开源使用、禁止商用——本仓库以同口径随应用
// 分发（署名见 desktop/src/builtinPacks.ts 与 README），**禁止用于商业用途**。
//
// 用法：
//   1. npm pack dsh-pet && tar -xzf dsh-pet-<ver>.tgz -C /tmp/dsh-pet-pkg
//   2. node scripts/build-dafeiyu-pack.mjs /tmp/dsh-pet-pkg/package/assets/webm
//
// 子集清单必须与 desktop/src/builtinPacks.ts 的 DA_FEI_YU_PACK 保持一致
// （那里是渲染真源，这里只负责把 webm 从源目录拷进 assets）。升级素材集：
// 两处同步修改 + 递增 BUILTIN_PACK_VERSION，老用户重启自动重装。

import { copyFileSync, mkdirSync, readdirSync } from 'node:fs'
import { resolve, join } from 'node:path'

const SOURCE_DIR = process.argv[2]
const DEST_DIR = resolve(import.meta.dirname, '../desktop/src/assets/packs/dafeiyu')

if (!SOURCE_DIR) {
  console.error('用法: node scripts/build-dafeiyu-pack.mjs <dsh-pet webm 目录>')
  process.exit(1)
}

// 与 builtinPacks.ts DA_FEI_YU_PACK 同步的素材子集（槽位 + actions）
const SELECTION = [
  '待机呼吸休闲', '螃蟹走路', '被鼠标拖拽悬空反馈', '原地小憩沉眠', '东张西望',
  '原地漂浮踏步', '原地左转奔跑',
  '点击回应-开心跃动', '点击回应-害羞惊讶', '点击回应-傲娇生气', '点击回应-挠痒咯咯笑', '点击回应-元气挥手',
  '悠闲哼歌', '超大伸懒腰', '哈欠连天', '女仆屈膝礼仪', '被吓一跳', '写代码', '晨间刷牙',
  '原地专心玩魔方', '鲸鱼吐泡泡特效', '优雅女仆舞', '可爱宅舞', '撸猫', '拆礼物', '荡秋千', '骑木马',
  '吃白饭', '大口吃零食', '吃Token', '涮火锅', '吃糖葫芦',
  '堆雪人', '放烟花', '舞狮头', '收红包',
  '是啊，吃什么',
  '工作状态-思考冒泡', '工作状态-忙碌点按', '工作状态-清点归档',
  '工作状态-原地踱步张望', '工作状态-雀跃庆祝', '工作状态-垂头叹气冒汗',
  '余额-钱袋满溢', '余额-金袋叮当', '余额-钱袋如常', '余额-数金皱眉',
  '余额-袋空如洗', '余额-分文不剩',
]

const available = new Set(readdirSync(SOURCE_DIR))
mkdirSync(DEST_DIR, { recursive: true })
let copied = 0
for (const name of SELECTION) {
  const src = join(SOURCE_DIR, `${name}.webm`)
  if (!available.has(`${name}.webm`)) {
    console.error(`源目录缺素材: ${name}.webm`)
    process.exit(1)
  }
  copyFileSync(src, join(DEST_DIR, `${name}.webm`))
  copied++
}
console.log(`已同步 ${copied}/${SELECTION.length} 条素材 → ${DEST_DIR}`)
console.log('记得核对 desktop/src/builtinPacks.ts 的 clips 清单与本脚本 SELECTION 一致。')
