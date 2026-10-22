// 生成测试宠物包：程序化画出「几何小人」的 idle / walk / 挥手 / 蹦跳 四个
// PNG 雪碧图片段 + pack.json，整包写入 ~/.jarvis/pets/test-pack/。
//
// 用法：node scripts/make-test-pet-pack.mjs
//
// 零第三方依赖：PNG 用 node:zlib 压缩 + 手写 CRC32/IHDR/IDAT/IEND 组装，
// 图形是逐像素画的几何小人（头圆 + 身体 + 四肢），透明底。仅用于联调与
// 人工验收，不打包进应用。

import zlib from 'node:zlib'
import fs from 'node:fs'
import path from 'node:path'

const FRAME = 64 // 单帧边长
const BODY = [74, 222, 128, 255]   // 绿
const HEAD = [96, 165, 250, 255]   // 蓝
const LIMB = [250, 204, 21, 255]   // 黄

// ===== 最小 PNG 编码器 =====

function crc32(buf) {
  let table = crc32.table
  if (!table) {
    table = crc32.table = new Int32Array(256)
    for (let n = 0; n < 256; n++) {
      let c = n
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
      table[n] = c
    }
  }
  let crc = -1
  for (let i = 0; i < buf.length; i++) crc = (crc >>> 8) ^ table[(crc ^ buf[i]) & 0xff]
  return (crc ^ -1) >>> 0
}

function pngChunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const typeBuf = Buffer.from(type, 'ascii')
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])))
  return Buffer.concat([len, typeBuf, data, crc])
}

function encodePNG(width, height, rgba) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(width, 0)
  ihdr.writeUInt32BE(height, 4)
  ihdr[8] = 8   // bit depth
  ihdr[9] = 6   // color type RGBA
  const stride = width * 4
  const raw = Buffer.alloc((stride + 1) * height)
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0 // filter: none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride)
  }
  return Buffer.concat([sig, pngChunk('IHDR', ihdr), pngChunk('IDAT', zlib.deflateSync(raw, { level: 9 })), pngChunk('IEND', Buffer.alloc(0))])
}

// ===== 逐像素画几何小人 =====

function makeCanvas(w, h) {
  return { w, h, px: Buffer.alloc(w * h * 4) }
}

function blendPixel(img, x, y, color) {
  if (x < 0 || y < 0 || x >= img.w || y >= img.h) return
  const i = (y * img.w + x) * 4
  img.px[i] = color[0]
  img.px[i + 1] = color[1]
  img.px[i + 2] = color[2]
  img.px[i + 3] = color[3]
}

function fillCircle(img, cx, cy, r, color) {
  const r2 = r * r
  for (let y = Math.floor(cy - r); y <= Math.ceil(cy + r); y++) {
    for (let x = Math.floor(cx - r); x <= Math.ceil(cx + r); x++) {
      const dx = x + 0.5 - cx
      const dy = y + 0.5 - cy
      if (dx * dx + dy * dy <= r2) blendPixel(img, x, y, color)
    }
  }
}

function fillRect(img, x, y, w, h, color) {
  for (let dy = 0; dy < h; dy++) {
    for (let dx = 0; dx < w; dx++) blendPixel(img, Math.round(x) + dx, Math.round(y) + dy, color)
  }
}

/** 粗线段：沿线密集撒圆点 */
function drawLimb(img, x0, y0, x1, y1, thickness, color) {
  const steps = Math.ceil(Math.hypot(x1 - x0, y1 - y0)) * 2 + 1
  for (let i = 0; i <= steps; i++) {
    const t = i / steps
    fillCircle(img, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, thickness / 2, color)
  }
}

/**
 * 画一帧小人。坐标系 64×64：头心 (32, 14)，躯干 (32,20)-(32,40)，脚底 y≈58。
 * @param pose { bodyDx, bodyDy, legPhase(0..1 走相位), armL, armR(手臂角度 rad),
 *               jump(跳跃抬升 px), tuck(收腿) }
 */
function drawFrame(pose) {
  const img = makeCanvas(FRAME, FRAME)
  const cx = 32 + (pose.bodyDx ?? 0)
  const bob = pose.bodyDy ?? 0
  const headY = 14 + bob - (pose.jump ?? 0)
  const torsoTop = 20 + bob - (pose.jump ?? 0)
  const hipY = 38 + bob - (pose.jump ?? 0)

  // 腿：walk 相位 0..1，两腿正反对摆；tuck 时收起
  const swing = Math.sin((pose.legPhase ?? 0) * Math.PI * 2) * 10
  const footLY = pose.tuck ? hipY + 8 : 58 - (pose.jump ?? 0)
  const footRY = pose.tuck ? hipY + 6 : 58 - (pose.jump ?? 0)
  drawLimb(img, cx, hipY, cx - swing, footLY, 4, LIMB)
  drawLimb(img, cx, hipY, cx + swing, footRY, 4, LIMB)

  // 躯干
  fillRect(img, cx - 5, torsoTop, 10, hipY - torsoTop + 2, BODY)

  // 手臂：默认垂下；armR/armL 为相对水平抬起的角度（负=朝上举）
  const armLen = 12
  const aL = pose.armL ?? Math.PI / 2
  const aR = pose.armR ?? Math.PI / 2
  drawLimb(img, cx, torsoTop + 3, cx - Math.cos(aL) * armLen, torsoTop + 3 + Math.sin(aL) * armLen, 4, LIMB)
  drawLimb(img, cx, torsoTop + 3, cx + Math.cos(aR) * armLen, torsoTop + 3 + Math.sin(aR) * armLen, 4, LIMB)

  // 头 + 眼睛（朝右看，pack.json facing 记 left → 走向左走时不翻转，走向右走翻转）
  fillCircle(img, cx, headY, 8, HEAD)
  fillRect(img, cx + 2, headY - 3, 2, 2, [15, 23, 42, 255])
  fillRect(img, cx + 6, headY - 3, 2, 2, [15, 23, 42, 255])
  return img
}

/** 横排雪碧图：frames 逐帧并排，columns = 帧数（单行） */
function makeSheet(frames) {
  const img = makeCanvas(FRAME * frames.length, FRAME)
  frames.forEach((f, i) => {
    f.px.copy(img.px, i * FRAME * FRAME * 4, 0, FRAME * FRAME * 4)
  })
  return encodePNG(img.w, img.h, img.px)
}

// ===== 四个片段 =====

// idle：原地站立轻微呼吸起伏
const idle = [0, 1, 2, 1].map(bodyDy => drawFrame({ bodyDy }))
// walk：6 帧步态循环 + 身体小幅起伏
const walk = [0, 1, 2, 3, 4, 5].map(i => drawFrame({
  legPhase: i / 6,
  bodyDy: i % 3 === 1 ? -1 : 0,
  armL: Math.PI / 2 + Math.sin((i / 6) * Math.PI * 2) * 0.5,
  armR: Math.PI / 2 - Math.sin((i / 6) * Math.PI * 2) * 0.5,
}))
// 挥手：右臂从平举到高举摆动
const wave = [0, 1, 2, 3].map(i => drawFrame({
  armR: -Math.PI / 2 + Math.sin((i / 4) * Math.PI * 2) * 0.6,
  bodyDy: i % 2 === 0 ? 0 : -1,
}))
// 蹦跳：起跳-腾空-落地-缓冲
const jump = [
  drawFrame({ jump: 0, bodyDy: 2 }),   // 蹲
  drawFrame({ jump: 10, tuck: true }), // 腾空收腿
  drawFrame({ jump: 4 }),              // 下落
  drawFrame({ jump: 0, bodyDy: 2 }),   // 落地缓冲
]

// ===== 组包落盘 =====

const sheet = (fps, frames) => ({
  type: 'spritesheet',
  frameW: FRAME,
  frameH: FRAME,
  fps,
  frames: frames.length,
  columns: frames.length,
})

const packJson = {
  id: 'test-pack',
  name: '测试几何小人',
  scale: 1.0,
  facing: 'left',
  clips: {
    idle: { file: 'idle.png', ...sheet(6, idle) },
    walk: { file: 'walk.png', ...sheet(10, walk) },
    actions: [
      { name: '挥手', file: 'wave.png', weight: 3, ...sheet(6, wave) },
      { name: '蹦跳', file: 'jump.png', weight: 1, ...sheet(8, jump) },
    ],
  },
}

const home = process.env.USERPROFILE || process.env.HOME
if (!home) {
  console.error('找不到用户目录（USERPROFILE/HOME 均为空）')
  process.exit(1)
}
const outDir = path.join(home, '.jarvis', 'pets', 'test-pack')
fs.mkdirSync(outDir, { recursive: true })
fs.writeFileSync(path.join(outDir, 'idle.png'), makeSheet(idle))
fs.writeFileSync(path.join(outDir, 'walk.png'), makeSheet(walk))
fs.writeFileSync(path.join(outDir, 'wave.png'), makeSheet(wave))
fs.writeFileSync(path.join(outDir, 'jump.png'), makeSheet(jump))
fs.writeFileSync(path.join(outDir, 'pack.json'), JSON.stringify(packJson, null, 2))
fs.writeFileSync(
  path.join(outDir, 'README.txt'),
  '用法：在项目根目录运行 node scripts/make-test-pet-pack.mjs 重新生成本测试包；素材可手动替换（同名覆盖后改 pack.json 对应字段）。\n',
)

console.log(`测试宠物包已生成: ${outDir}`)
console.log('片段: idle(4帧) / walk(6帧) / 挥手(4帧) / 蹦跳(4帧)，全部为 PNG 雪碧图')
console.log('在应用「设置 → 宠物形象」里重新打开设置窗口即可在下拉「宠物包」分组看到「测试几何小人」')
