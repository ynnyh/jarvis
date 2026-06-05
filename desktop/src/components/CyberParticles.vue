<script setup lang="ts">
// 未来战「颗粒朦胧」背景：自包含 canvas + 静态星光层，仅当 styleTheme === 'cyber' 时渲染并跑动画。
//
// 设计要点：
//  - 静态星光层（CSS）：radial-gradient 多层叠加，模拟深空星空
//  - 动态颗粒层（Canvas）：稀疏、缓慢、闪烁的浮动颗粒（比 matrix 雨少、慢、淡）
//  - 主题绑定：v-if="isCyber"，非 cyber 主题不渲染，节省 CPU
//  - 节流到 ~24fps，并在标签页隐藏时彻底停 RAF
//  - ResizeObserver 适配父容器尺寸变化
//
// 能量粒子效果（Pulsefire Ezreal 风格）：
//  - 运动拖尾：半透明背景填充替代 clearRect，粒子移动留下发光残影
//  - 粒子连线：距离 <120px 的粒子之间绘制低透明度连线，形成能量网络
//  - 脉冲突发：每 4-6 秒随机位置爆发一组高亮度粒子，逐渐减速融入常态
//  - 速度呼吸：每个粒子的速度按 sin 曲线周期性波动，产生有机的加减速节奏

import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useConfigStore } from '../stores/config'

const store = useConfigStore()
const isCyber = computed(() => store.config.styleTheme === 'cyber')

const canvasRef = ref<HTMLCanvasElement | null>(null)

// —— 动画常量 ——
const FRAME_MS = 1000 / 24     // 节流到 ~24fps（连线需要稍高帧率）
const PARTICLE_COLORS = ['#00d4ff', '#00e5ff', '#a855f7', '#d8b4fe'] // 电光蓝×全息紫
const TRAIL_BG = 'rgba(5, 10, 20, 0.15)'  // 拖尾半透明填充（cyber 主题底色 + alpha）
const CONNECTION_DIST = 120    // 连线最大距离
const CONNECTION_MAX_CHECK = 80 // 连线性能上限：只检查前 N 个粒子
const BURST_INTERVAL_MIN = 4000 // 脉冲突发最小间隔 ms
const BURST_INTERVAL_MAX = 6000 // 脉冲突发最大间隔 ms
const BURST_COUNT_MIN = 8      // 单次爆发最少粒子数
const BURST_COUNT_MAX = 15     // 单次爆发最多粒子数
const BURST_DECAY = 0.97       // 爆发粒子减速系数

interface Particle {
  x: number
  y: number
  vx: number
  vy: number
  baseVx: number               // 基础速度（呼吸曲线的振幅基准）
  baseVy: number
  size: number
  color: string
  alpha: number
  alphaSpeed: number
  phase: number                // 速度呼吸相位 0..2π
  phaseSpeed: number           // 相位递增速度（每帧）
  isBurst: boolean             // 是否为脉冲突发粒子
}

let ctx: CanvasRenderingContext2D | null = null
let rafId = 0
let lastTime = 0
let particles: Particle[] = []
let resizeObserver: ResizeObserver | null = null

// —— 脉冲突发状态 ——
let nextBurstTime = 0  // 下一次突发的绝对时间戳 ms

function scheduleBurst(now: number) {
  nextBurstTime = now + BURST_INTERVAL_MIN + Math.random() * (BURST_INTERVAL_MAX - BURST_INTERVAL_MIN)
}

function randomColor(): string {
  return PARTICLE_COLORS[Math.floor(Math.random() * PARTICLE_COLORS.length)]
}

function createParticle(width: number, height: number): Particle {
  const vx = (Math.random() - 0.5) * 0.3
  const vy = Math.random() * 0.4 + 0.1
  return {
    x: Math.random() * width,
    y: Math.random() * height,
    vx,
    vy,
    baseVx: vx,
    baseVy: vy,
    size: Math.random() * 1.5 + 0.5, // 小颗粒 0.5-2px
    color: randomColor(),
    alpha: Math.random() * 0.5 + 0.3,
    alphaSpeed: (Math.random() - 0.5) * 0.02, // 闪烁速度
    phase: Math.random() * Math.PI * 2,        // 随机初始相位
    phaseSpeed: Math.random() * 0.01 + 0.005,  // 相位递增 0.005-0.015
    isBurst: false,
  }
}

// 按容器尺寸重算 canvas 宽高与颗粒数量（宽度 / 40px，比 matrix 少很多）
function resize() {
  const canvas = canvasRef.value
  if (!canvas) return
  const parent = canvas.parentElement
  const w = parent?.clientWidth ?? canvas.clientWidth
  const h = parent?.clientHeight ?? canvas.clientHeight
  if (w <= 0 || h <= 0) return
  canvas.width = w
  canvas.height = h

  const targetCount = Math.max(10, Math.floor(w / 40)) // 颗粒数约为宽度 / 40
  if (particles.length < targetCount) {
    while (particles.length < targetCount) {
      particles.push(createParticle(w, h))
    }
  } else if (particles.length > targetCount) {
    particles = particles.slice(0, targetCount)
  }
}

// 脉冲突发：在随机中心点生成一批高亮度粒子
function spawnBurst(width: number, height: number) {
  const cx = Math.random() * width
  const cy = Math.random() * height
  const count = BURST_COUNT_MIN + Math.floor(Math.random() * (BURST_COUNT_MAX - BURST_COUNT_MIN + 1))
  for (let i = 0; i < count; i++) {
    const angle = Math.random() * Math.PI * 2
    const speed = (Math.random() * 2 + 2) // 2-4x 正常态速度
    const vx = Math.cos(angle) * speed
    const vy = Math.sin(angle) * speed
    particles.push({
      x: cx,
      y: cy,
      vx,
      vy,
      baseVx: vx,
      baseVy: vy,
      size: Math.random() * 2 + 2,  // 2-4px，比常态大
      color: randomColor(),
      alpha: Math.random() * 0.4 + 0.6, // 0.6-1.0，高亮
      alphaSpeed: 0,  // 爆发粒子不闪烁，直接减速衰减
      phase: 0,       // 爆发粒子不用呼吸曲线
      phaseSpeed: 0,
      isBurst: true,
    })
  }
}

function drawFrame(time: number) {
  const canvas = canvasRef.value
  if (!ctx || !canvas) return
  const w = canvas.width
  const h = canvas.height

  // —— 拖尾：半透明底色覆盖替代 clearRect ——
  ctx.globalAlpha = 1
  ctx.fillStyle = TRAIL_BG
  ctx.fillRect(0, 0, w, h)

  // —— 脉冲突发检测 ——
  if (time >= nextBurstTime) {
    spawnBurst(w, h)
    scheduleBurst(time)
  }

  // —— 更新所有粒子状态 ——
  for (const p of particles) {
    if (p.isBurst) {
      // 爆发粒子：减速 + 淡出
      p.vx *= BURST_DECAY
      p.vy *= BURST_DECAY
      p.alpha *= 0.985  // 缓慢淡出
      // 减速到接近静止时标记淡出（alpha < 0.05 会在绘制时跳过）
    } else {
      // 常态粒子：速度呼吸曲线
      p.phase += p.phaseSpeed
      const breath = 0.5 + 0.5 * Math.sin(p.phase)
      p.vx = p.baseVx * breath
      p.vy = p.baseVy * breath
      p.alpha += p.alphaSpeed
      // 闪烁边界：淡入淡出
      if (p.alpha <= 0.2 || p.alpha >= 0.8) {
        p.alphaSpeed = -p.alphaSpeed
      }
    }

    // 更新位置
    p.x += p.vx
    p.y += p.vy

    // 边界处理：超出后从对面进入
    if (p.x < 0) p.x = w
    if (p.x > w) p.x = 0
    if (p.y > h) {
      p.y = 0
      p.x = Math.random() * w
    }
  }

  // —— 清理已淡出的爆发粒子 ——
  particles = particles.filter(p => !p.isBurst || p.alpha >= 0.05)

  // —— 粒子连线（能量网络）——
  // 只检查前 CONNECTION_MAX_CHECK 个粒子，避免 O(n²) 性能爆炸
  const checkCount = Math.min(particles.length, CONNECTION_MAX_CHECK)
  ctx.lineWidth = 0.5
  for (let i = 0; i < checkCount; i++) {
    for (let j = i + 1; j < checkCount; j++) {
      const a = particles[i]
      const b = particles[j]
      const dx = a.x - b.x
      const dy = a.y - b.y
      const dist = Math.sqrt(dx * dx + dy * dy)
      if (dist < CONNECTION_DIST) {
        // 距离越近越不透明，最远时趋近 0
        const lineAlpha = (1 - dist / CONNECTION_DIST) * 0.15
        ctx.globalAlpha = lineAlpha
        ctx.strokeStyle = '#00d4ff'
        ctx.beginPath()
        ctx.moveTo(a.x, a.y)
        ctx.lineTo(b.x, b.y)
        ctx.stroke()
      }
    }
  }

  // —— 绘制所有粒子 ——
  for (const p of particles) {
    const a = Math.max(0, Math.min(1, p.alpha))
    if (a < 0.02) continue  // 几乎透明则跳过
    ctx.globalAlpha = a
    ctx.fillStyle = p.color
    ctx.beginPath()
    ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2)
    ctx.fill()
  }
}

function tick(time: number) {
  if (!isCyber.value || document.hidden) {
    rafId = 0
    return
  }
  rafId = requestAnimationFrame(tick)
  if (time - lastTime < FRAME_MS) return
  lastTime = time
  drawFrame(time)
}

function start() {
  const canvas = canvasRef.value
  if (!canvas) return
  ctx = canvas.getContext('2d')
  if (!ctx) return
  resize()
  if (!resizeObserver && canvas.parentElement) {
    resizeObserver = new ResizeObserver(() => resize())
    resizeObserver.observe(canvas.parentElement)
  }
  lastTime = 0
  scheduleBurst(performance.now())  // 安排第一次脉冲突发
  if (!rafId) rafId = requestAnimationFrame(tick)
}

function stop() {
  if (rafId) {
    cancelAnimationFrame(rafId)
    rafId = 0
  }
}

// 主题非 cyber 时停并清空 canvas（释放画面 + 不占 CPU）
function clearCanvas() {
  const canvas = canvasRef.value
  if (ctx && canvas) ctx.clearRect(0, 0, canvas.width, canvas.height)
}

// 从隐藏恢复时，先填充底色清除拖尾残影，避免恢复瞬间画面脏
function resetCanvasBackground() {
  const canvas = canvasRef.value
  if (ctx && canvas) {
    ctx.globalAlpha = 1
    ctx.fillStyle = TRAIL_BG
    ctx.fillRect(0, 0, canvas.width, canvas.height)
  }
}

function onVisibilityChange() {
  if (!isCyber.value) return
  if (document.hidden) {
    stop()
  } else if (!rafId) {
    resetCanvasBackground()  // 恢复时清除拖尾残影
    lastTime = 0
    scheduleBurst(performance.now())  // 重新安排突发
    rafId = requestAnimationFrame(tick)
  }
}

// isCyber 切换：v-if 让 canvas 出现/消失，DOM 就绪后再 start。
watch(isCyber, (on) => {
  if (on) {
    // 等 v-if 渲染出 canvas
    requestAnimationFrame(() => start())
  } else {
    stop()
    clearCanvas()
  }
})

onMounted(() => {
  document.addEventListener('visibilitychange', onVisibilityChange)
  if (isCyber.value) start()
})

onBeforeUnmount(() => {
  document.removeEventListener('visibilitychange', onVisibilityChange)
  stop()
  if (resizeObserver) {
    resizeObserver.disconnect()
    resizeObserver = null
  }
  ctx = null
})
</script>

<template>
  <div v-if="isCyber" class="cyber-particles" aria-hidden="true">
    <!-- 静态星光层（CSS） -->
    <div class="stars" />
    <!-- 动态颗粒层（Canvas） -->
    <canvas ref="canvasRef" class="particles-canvas" />
  </div>
</template>

<style scoped>
.cyber-particles {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  z-index: -1;
  pointer-events: none;
}

.stars {
  position: absolute;
  inset: 0;
  background-image:
    radial-gradient(1px 1px at 10% 10%, rgba(0,212,255,0.8) 50%, transparent 50%),
    radial-gradient(1px 1px at 20% 30%, rgba(168,85,247,0.6) 50%, transparent 50%),
    radial-gradient(1.5px 1.5px at 35% 15%, rgba(0,229,255,0.5) 50%, transparent 50%),
    radial-gradient(1px 1px at 50% 45%, rgba(0,212,255,0.7) 50%, transparent 50%),
    radial-gradient(1px 1px at 65% 70%, rgba(216,180,254,0.6) 50%, transparent 50%),
    radial-gradient(1.5px 1.5px at 80% 25%, rgba(0,212,255,0.4) 50%, transparent 50%),
    radial-gradient(1px 1px at 90% 85%, rgba(168,85,247,0.8) 50%, transparent 50%),
    radial-gradient(1px 1px at 15% 75%, rgba(0,229,255,0.5) 50%, transparent 50%),
    radial-gradient(1px 1px at 40% 60%, rgba(216,180,254,0.7) 50%, transparent 50%),
    radial-gradient(1.5px 1.5px at 70% 40%, rgba(0,212,255,0.6) 50%, transparent 50%);
  background-size: 200px 200px;
  opacity: 0.4;
}

.particles-canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}
</style>
