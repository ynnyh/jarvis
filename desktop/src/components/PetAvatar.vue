<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, shallowRef, type CSSProperties } from 'vue'
import lottie, { type AnimationItem } from 'lottie-web'
import { getPetById } from '../petManifest'

// 单一职责：把选中的宠物渲染到一个 72×72 透明容器里，
// 外面套一圈跟当前 state 联动的发光环 + 右下角状态点。换 petId 时销毁旧
// 动画再加载新动画，避免内存堆积。
//
// 支持三种宠物类型：
// - lottie: 使用 lottie-web 渲染 Lottie JSON
// - image: 使用 <img> 标签，可选 CSS 动画
// - gif: 使用 <img> 标签，GIF 自带动画

const props = defineProps<{
  petId: string
  color: string       // 当前 state 的主色
  glowColor: string   // 当前 state 的发光色（半透明 rgba）
  active: boolean     // working 态时高亮
  flashing: boolean   // 状态切换瞬间脉冲一次
}>()

const containerEl = ref<HTMLDivElement | null>(null)
const imgEl = ref<HTMLImageElement | null>(null)
// shallowRef 避免 Vue 把 lottie 的 AnimationItem 当作普通对象深度代理 —— 它内部
// 持有 SVGElement / 帧数据，被 proxy 包装可能造成 # 私有字段访问失败或性能问题。
const animation = shallowRef<AnimationItem | null>(null)

const pet = computed(() => getPetById(props.petId))
const renderConfig = computed(() => pet.value.render ?? {})
const isLottie = computed(() => !pet.value.petType || pet.value.petType === 'lottie')
const isImage = computed(() => pet.value.petType === 'image')
const isGif = computed(() => pet.value.petType === 'gif')
const isMedia = computed(() => isImage.value || isGif.value)

const lottieStyle = computed<CSSProperties>(() => ({
  '--pet-scale': String(renderConfig.value.scale ?? 1),
  '--pet-offset-x': `${renderConfig.value.offsetX ?? 0}px`,
  '--pet-offset-y': `${renderConfig.value.offsetY ?? 0}px`,
}))

const mediaStyle = computed<CSSProperties>(() => ({
  '--pet-scale': String(renderConfig.value.scale ?? 1),
  '--pet-offset-x': `${renderConfig.value.offsetX ?? 0}px`,
  '--pet-offset-y': `${renderConfig.value.offsetY ?? 0}px`,
}))

const imageAnimationClass = computed(() => {
  if (!isImage.value) return ''
  const anim = pet.value.imageAnimation
  if (anim === 'breath') return 'anim-breath'
  if (anim === 'swing') return 'anim-swing'
  if (anim === 'rotate') return 'anim-rotate'
  if (anim === 'bounce') return 'anim-bounce'
  return ''
})

/** 图片/GIF 的 Base64 data URL */
const mediaSrc = computed(() => {
  if (!isMedia.value) return ''
  const data = pet.value.data
  if (typeof data === 'string') {
    // 已经是 data URL 或 base64 字符串
    return data.startsWith('data:') ? data : `data:image/png;base64,${data}`
  }
  return ''
})

function mountAnimation() {
  if (!containerEl.value || !isLottie.value) return
  if (animation.value) {
    animation.value.destroy()
    animation.value = null
  }
  animation.value = lottie.loadAnimation({
    container: containerEl.value,
    renderer: 'svg',
    loop: true,
    autoplay: true,
    animationData: pet.value.data,
  })
}

onMounted(() => {
  mountAnimation()
})

onBeforeUnmount(() => {
  if (animation.value) {
    animation.value.destroy()
    animation.value = null
  }
})

watch(() => props.petId, () => {
  mountAnimation()
})
</script>

<template>
  <div class="pet-avatar" :class="{ active, flashing }">
    <div
      class="pet-glow"
      :style="{ background: glowColor, boxShadow: `0 0 18px ${color}55, 0 0 36px ${color}33` }"
    />
    <div v-if="isLottie" ref="containerEl" class="pet-lottie" :style="lottieStyle" />
    <img
      v-else-if="isMedia"
      ref="imgEl"
      class="pet-media"
      :class="imageAnimationClass"
      :style="mediaStyle"
      :src="mediaSrc"
      alt=""
    />
    <div class="pet-dot" :style="{ background: color }" />
  </div>
</template>

<style scoped>
.pet-avatar {
  position: relative;
  width: 72px;
  height: 72px;
  cursor: pointer;
  transition: transform 0.18s ease;
}
.pet-avatar:hover { transform: scale(1.08); }

.pet-glow {
  position: absolute;
  inset: -2px;
  border-radius: 50%;
  z-index: 0;
  animation: pet-glow-pulse 2.4s ease-in-out infinite;
}
@keyframes pet-glow-pulse {
  0%, 100% { opacity: 0.4; }
  50% { opacity: 0.85; }
}

.pet-lottie {
  position: absolute;
  inset: 0;
  z-index: 1;
  /* Lottie 渲染出来的 SVG 默认会带边距和 viewBox 留白；用 transform: scale
     让动画稍微撑大一点，72×72 容器内更饱满。具体倍数靠经验，1.1 视觉刚好。 */
  display: flex;
  align-items: center;
  justify-content: center;
  transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1));
  transform-origin: center center;
}
.pet-lottie :deep(svg) {
  width: 100% !important;
  height: 100% !important;
}

.pet-dot {
  position: absolute;
  bottom: 3px;
  right: 3px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  border: 2px solid rgba(15, 23, 42, 1);
  z-index: 2;
}

/* 图片/GIF 宠物 */
.pet-media {
  position: absolute;
  inset: 4px;
  z-index: 1;
  width: calc(100% - 8px);
  height: calc(100% - 8px);
  object-fit: cover;
  border-radius: 50%;
  box-shadow: 0 0 8px rgba(0, 0, 0, 0.3);
  transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1));
  transform-origin: center center;
}

/* 图片动画：呼吸效果 */
.anim-breath {
  animation: pet-breath 3s ease-in-out infinite;
}
@keyframes pet-breath {
  0%, 100% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)); }
  50% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(calc(var(--pet-scale, 1) * 1.08)); }
}

/* 图片动画：摇摆效果 */
.anim-swing {
  animation: pet-swing 2.5s ease-in-out infinite;
}
@keyframes pet-swing {
  0%, 100% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(0deg); }
  25% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(5deg); }
  75% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(-5deg); }
}

/* 图片动画：旋转效果 */
.anim-rotate {
  animation: pet-rotate 8s linear infinite;
}
@keyframes pet-rotate {
  0% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(0deg); }
  100% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(360deg); }
}

/* 图片动画：弹跳效果 */
.anim-bounce {
  animation: pet-bounce 1.5s ease-in-out infinite;
}
@keyframes pet-bounce {
  0%, 100% { transform: translate(var(--pet-offset-x, 0), calc(var(--pet-offset-y, 0) + 0px)) scale(var(--pet-scale, 1)); }
  50% { transform: translate(var(--pet-offset-x, 0), calc(var(--pet-offset-y, 0) - 6px)) scale(var(--pet-scale, 1)); }
}

/* 状态切换瞬间脉冲一次，给视觉信号 */
.pet-avatar.flashing .pet-glow {
  animation: pet-glow-pulse 2.4s ease-in-out infinite, pet-flash 0.4s ease-out 0s 2;
}
@keyframes pet-flash {
  0%   { transform: scale(1);    opacity: 1; }
  50%  { transform: scale(1.45); opacity: 0.4; }
  100% { transform: scale(1);    opacity: 1; }
}
</style>
