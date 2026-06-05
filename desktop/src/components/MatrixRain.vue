<script setup lang="ts">
// 黑客帝国「数字雨」背景：自包含 canvas，仅当 styleTheme === 'matrix' 时渲染并跑动画。
//
// 设计要点：
//  - 它是固定画面元素（绿色雨），不走主题 token，字符颜色直接硬编码绿；UI 其它颜色仍由 token 管。
//  - 绝对铺满父容器、置于内容之下（z-index:0）、不挡交互（pointer-events:none）。
//    父窗口根 `.theme-bg` 已是 position:relative;z-index:0，内容在更高层，故本层在背景。
//  - 节流到 ~30fps（时间戳判断），并在标签页隐藏 / 主题非 matrix 时彻底停 RAF，省 CPU。
//  - ResizeObserver 适配父容器尺寸变化，重算 canvas 宽高与列数。
//  - 像素尺寸按容器 clientWidth/clientHeight 设置（不做 devicePixelRatio 缩放，保持简单省性能）。

import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useConfigStore } from '../stores/config'

const store = useConfigStore()
const isMatrix = computed(() => store.config.styleTheme === 'matrix')

const canvasRef = ref<HTMLCanvasElement | null>(null)

// —— 动画常量 ——
const FONT_SIZE = 14          // 列宽 / 字号（px）
const HEAD_COLOR = '#88ff88'  // 列头字符（压暗，降低视觉干扰）
const TRAIL_COLOR = '#00ff41' // 矩阵绿（其余字符）
const FADE = 'rgba(0,0,0,0.22)' // 每帧半透明黑整屏填充 → 拖尾（0.22 拖尾更快散，视觉杂讯更少）
const FRAME_MS = 1000 / 30     // 节流到 ~30fps
const RESET_CHANCE = 0.975    // drop 越界后按概率重置到顶部（值越大尾巴越长）
const GLYPHS = 'ｱｲｳｴｵｶｷｸｹｺABCDEFG0123456789@#$%'
const RAIN_ALPHA = 0.25       // 整层雨极淡（接近水印，几乎不抢前景）

let ctx: CanvasRenderingContext2D | null = null
let rafId = 0
let lastTime = 0
let columns = 0
let drops: number[] = []        // 每列当前字符行的 y（以行为单位）
let resizeObserver: ResizeObserver | null = null

function randomGlyph(): string {
  return GLYPHS.charAt(Math.floor(Math.random() * GLYPHS.length))
}

// 按容器尺寸重算 canvas 像素宽高与列数；尽量保留已有 drops 让切换/缩放不突兀。
function resize() {
  const canvas = canvasRef.value
  if (!canvas) return
  const parent = canvas.parentElement
  const w = parent?.clientWidth ?? canvas.clientWidth
  const h = parent?.clientHeight ?? canvas.clientHeight
  if (w <= 0 || h <= 0) return
  canvas.width = w
  canvas.height = h
  columns = Math.max(1, Math.ceil(w / FONT_SIZE))
  const next = new Array(columns)
  for (let i = 0; i < columns; i++) {
    // 复用旧值，新列从随机高度起，避免整齐第一帧
    next[i] = drops[i] ?? Math.floor((Math.random() * h) / FONT_SIZE)
  }
  drops = next
}

function drawFrame() {
  const canvas = canvasRef.value
  if (!ctx || !canvas) return
  // 半透明黑整屏填充制造拖尾
  ctx.fillStyle = FADE
  ctx.fillRect(0, 0, canvas.width, canvas.height)
  ctx.globalAlpha = RAIN_ALPHA  // 整层雨半透明，避免抢前景
  ctx.font = `${FONT_SIZE}px monospace`

  const maxRows = canvas.height / FONT_SIZE
  for (let i = 0; i < columns; i++) {
    const x = i * FONT_SIZE
    const y = drops[i] * FONT_SIZE
    // 列头更亮，尾部矩阵绿
    ctx.fillStyle = HEAD_COLOR
    ctx.fillText(randomGlyph(), x, y)
    // 紧随其后画一个暗绿，强化拖尾观感
    if (drops[i] > 1) {
      ctx.fillStyle = TRAIL_COLOR
      ctx.fillText(randomGlyph(), x, y - FONT_SIZE)
    }
    // 越界后按概率重置到顶部
    if (y > canvas.height && Math.random() > RESET_CHANCE) {
      drops[i] = 0
    } else {
      drops[i] += 1
    }
    void maxRows
  }
}

function tick(time: number) {
  if (!isMatrix.value || document.hidden) {
    rafId = 0
    return
  }
  rafId = requestAnimationFrame(tick)
  if (time - lastTime < FRAME_MS) return
  lastTime = time
  drawFrame()
}

function start() {
  const canvas = canvasRef.value
  if (!canvas) return
  ctx = canvas.getContext('2d')
  if (!ctx) return
  resize()
  // 黑底起步，避免首帧透明闪白
  ctx.fillStyle = '#000'
  ctx.fillRect(0, 0, canvas.width, canvas.height)
  if (!resizeObserver && canvas.parentElement) {
    resizeObserver = new ResizeObserver(() => resize())
    resizeObserver.observe(canvas.parentElement)
  }
  lastTime = 0
  if (!rafId) rafId = requestAnimationFrame(tick)
}

function stop() {
  if (rafId) {
    cancelAnimationFrame(rafId)
    rafId = 0
  }
}

// 主题非 matrix 时停并清空 canvas（释放画面 + 不占 CPU）
function clearCanvas() {
  const canvas = canvasRef.value
  if (ctx && canvas) ctx.clearRect(0, 0, canvas.width, canvas.height)
}

function onVisibilityChange() {
  if (!isMatrix.value) return
  if (document.hidden) {
    stop()
  } else if (!rafId) {
    lastTime = 0
    rafId = requestAnimationFrame(tick)
  }
}

// isMatrix 切换：v-if 让 canvas 出现/消失，DOM 就绪后再 start。
watch(isMatrix, (on) => {
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
  if (isMatrix.value) start()
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
  <canvas v-if="isMatrix" ref="canvasRef" class="matrix-rain" aria-hidden="true" />
</template>

<style scoped>
.matrix-rain {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  z-index: -1;
  pointer-events: none;
}
</style>
