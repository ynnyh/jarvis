// 内置「大肥鱼」素材转换：dsh-pet webm → 动画 WebP（img 路径播放）。
//
// 为什么转 WebP（2026-10-28）：webm 的 VP9-alpha 在 WebView2 视频合成路径上
// 有两宗罪——①dsh-pet 素材背景 alpha 残留 1/255（编码量化），视频层合成放大后
// 浅色壁纸上肉眼可见「方框」；②视频走独立交换链，透明窗上的逐像素 alpha 依赖
// 实验性行为。动画 WebP 走 <img> 普通图片合成，alpha 解码精确，转换时顺手把
// <4 的 alpha 残留钳到 0，方框根除。规格：384×216@10fps q70（渲染上限 ~504×283，76% 重采样，可接受），
// 全量约 26MB。体积再失控优先降 fps。
//
// 用法（webm 源目录 = dsh-pet npm 包 assets/webm）：
//   node scripts/convert-dafeiyu-webp.mjs <webm源目录>
// 产物：assets/packs/dafeiyu/*.webp（替换同名 .webm）+ durations.json（一次性
// 片段时长，供 img 路径的收尾定时器用——img 没有 ended 事件）。
// ffmpeg 路径可用 FFMPEG 环境变量覆盖（默认 C:/Users/82673/bin/ffmpeg.exe）。

import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { resolve, join, basename } from 'node:path'

const SRC_DIR = process.argv[2]
const FFMPEG = process.env.FFMPEG ?? 'C:/Users/82673/bin/ffmpeg.exe'
const DEST = resolve(import.meta.dirname, '../desktop/src/assets/packs/dafeiyu')

if (!SRC_DIR) {
  console.error('用法: node scripts/convert-dafeiyu-webp.mjs <dsh-pet webm 源目录>')
  process.exit(1)
}

// 与 builtinPacks.ts DA_FEI_YU_PACK 引用的文件名保持一致（去扩展名比对）
const needed = new Set(
  readdirSync(DEST).filter(f => f.endsWith('.webm')).map(f => basename(f, '.webm')),
)
if (needed.size === 0) {
  console.error('assets 目录里没有 .webm（先跑 build-dafeiyu-pack.mjs 同步源素材）')
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
    '-vf', "format=yuva420p,fps=10,split[main][alpha];[alpha]alphaextract,lut=y='if(lt(val,4),0,val)'[am];[main][am]alphamerge,scale=384:-2,format=rgba",
    '-c:v', 'libwebp_anim', '-lossless', '0', '-q:v', '80',
    '-compression_level', '4', '-loop', '0', '-an',
    out,
  ], { stdio: ['ignore', 'ignore', 'pipe'] })

  totalIn += statSync(src).size
  totalOut += statSync(out).size
  // 源 webm 完成使命（npm 包是最终来源），从 assets 移除避免仓库双份
  rmSync(join(DEST, `${name}.webm`))
}

writeFileSync(join(DEST, 'durations.json'), JSON.stringify(durations, null, 1))
console.log(`转换 ${needed.size} 条：webm ${(totalIn / 1048576).toFixed(1)}MB → webp ${(totalOut / 1048576).toFixed(1)}MB`)
