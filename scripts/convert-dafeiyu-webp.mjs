// 内置「大肥鱼」素材转换：dsh-pet webm → 动画 WebP（img 路径播放）。
//
// 为什么转 WebP（2026-10-28）：webm 的 VP9-alpha 在 WebView2 视频合成路径上
// 有两宗罪——①dsh-pet 素材背景 alpha 残留 1/255（编码量化），视频层合成放大后
// 浅色壁纸上肉眼可见「方框」；②视频走独立交换链，透明窗上的逐像素 alpha 依赖
// 实验性行为。动画 WebP 走 <img> 普通图片合成，alpha 解码精确，转换时顺手把
// <4 的 alpha 残留钳到 0，方框根除。
//
// 为什么 640×360（2026-11-02，从 384×216 提上来）：渲染链是 --pet-size 命中盒
// （inset 4px 后 ~108px）被 pack scale ×3 放大 → 116px 档实际显示 ~324 CSS px；
// 在 150% DPI 屏（常见副屏）需要 ~486 物理像素，384 素材被拉大 27% 全程发虚
// （真机实拍：呆毛/眼高光/蕾丝描边糊）。640 是 dsh-pet 源生分辨率，零重采样，
// 150% 屏反而 0.76 缩小采样，各档尺寸都锐。实测三档对比：384 明显糊 / 512 清楚
// 改善 / 640 最锐。体积代价 ~38MB → ~75MB，q72+compression_level 6 比 q80 省
// 13% 无肉眼损失。fps 维持 10（体积最大杠杆，动作流畅度另议）。
//
// 用法（webm 源目录 = dsh-pet npm 包 assets/webm）：
//   node scripts/convert-dafeiyu-webp.mjs <webm源目录>
// 产物：assets/packs/dafeiyu/*.webp（替换同名 .webm）+ durations.json（一次性
// 片段时长，供 img 路径的收尾定时器用——img 没有 ended 事件）。
// ffmpeg 路径可用 FFMPEG 环境变量覆盖（默认 C:/Users/82673/bin/ffmpeg.exe）。

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { resolve, join, basename } from 'node:path'

const SRC_DIR = process.argv[2]
const FFMPEG = process.env.FFMPEG ?? 'C:/Users/82673/bin/ffmpeg.exe'
const DEST = resolve(import.meta.dirname, '../desktop/src/assets/packs/dafeiyu')

if (!SRC_DIR) {
  console.error('用法: node scripts/convert-dafeiyu-webp.mjs <dsh-pet webm 源目录>')
  process.exit(1)
}

// 素材清单：优先以 assets 里的 .webm 反推（build-dafeiyu-pack 刚同步完的原始
// 流程）；assets 已是纯 webp 时（本仓库现状）退回 durations.json 的 key——
// 两个来源都与 DA_FEI_YU_PACK 引用的文件名一一对应。
const webmInDest = readdirSync(DEST).filter(f => f.endsWith('.webm'))
const durationsPrev = JSON.parse(readFileSync(join(DEST, 'durations.json'), 'utf8'))
const needed = webmInDest.length > 0
  ? webmInDest.map(f => basename(f, '.webm'))
  : Object.keys(durationsPrev).map(f => basename(f, '.webp'))
if (needed.length === 0) {
  console.error('没有素材清单：assets 里既无 .webm，durations.json 也为空')
  process.exit(1)
}

const durations = {}
let totalIn = 0
let totalOut = 0

for (const name of needed) {
  const src = join(SRC_DIR, `${name}.webm`)
  if (!existsSync(src)) {
    console.error(`源缺失: ${name}.webm`)
    process.exit(1)
  }
  // 时长探测（img 路径无 ended 事件，一次性片段靠 durationMs 收尾）。
  // ffmpeg -i 不带输出文件必然退 1，信息在 stderr——从异常里取。
  let probe = ''
  try {
    execFileSync(FFMPEG, ['-hide_banner', '-i', src], { stdio: ['ignore', 'ignore', 'pipe'] })
  } catch (e) {
    probe = e.stderr?.toString() ?? ''
  }
  const m = probe.match(/Duration: (\d+):(\d+):([\d.]+)/)
  const durationMs = m ? Math.round((+m[1] * 3600 + +m[2] * 60 + +m[3]) * 1000) : 4500
  durations[`${name}.webp`] = durationMs

  const out = join(DEST, `${name}.webp`)
  execFileSync(FFMPEG, [
    '-hide_banner', '-y',
    '-c:v', 'libvpx-vp9', '-i', src,
    '-vf', "format=yuva420p,fps=10,split[main][alpha];[alpha]alphaextract,lut=y='if(lt(val,4),0,val)'[am];[main][am]alphamerge,scale=640:-2,format=rgba",
    '-c:v', 'libwebp_anim', '-lossless', '0', '-q:v', '72',
    '-compression_level', '6', '-loop', '0', '-an',
    out,
  ], { stdio: ['ignore', 'ignore', 'pipe'] })

  totalIn += statSync(src).size
  totalOut += statSync(out).size
  // 源 webm 完成使命（npm 包是最终来源），从 assets 移除避免仓库双份
  // （assets 已是纯 webp 时无 webm 可删，force 兜住不报错）
  rmSync(join(DEST, `${name}.webm`), { force: true })
}

writeFileSync(join(DEST, 'durations.json'), JSON.stringify(durations, null, 1))
console.log(`转换 ${needed.length} 条：webm ${(totalIn / 1048576).toFixed(1)}MB → webp ${(totalOut / 1048576).toFixed(1)}MB`)
