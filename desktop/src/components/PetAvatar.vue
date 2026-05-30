<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, shallowRef, type CSSProperties } from 'vue'
import lottie, { type AnimationItem } from 'lottie-web'
import { getPetById } from '../petManifest'

// 单一职责：把选中的宠物 Lottie 渲染到一个 72×72 透明容器里，
// 外面套一圈跟当前 state 联动的发光环 + 右下角状态点。换 petId 时销毁旧
// 动画再加载新动画，避免内存堆积。
//
// 不在这里做拖拽 / 点击 / 状态判断 —— 那些归 App.vue 管，PetAvatar 只关心"长什么样"。

const props = defineProps<{
  petId: string
  color: string       // 当前 state 的主色
  glowColor: string   // 当前 state 的发光色（半透明 rgba）
  active: boolean     // working 态时高亮
  flashing: boolean   // 状态切换瞬间脉冲一次
}>()

const containerEl = ref<HTMLDivElement | null>(null)
// shallowRef 避免 Vue 把 lottie 的 AnimationItem 当作普通对象深度代理 —— 它内部
// 持有 SVGElement / 帧数据，被 proxy 包装可能造成 # 私有字段访问失败或性能问题。
const animation = shallowRef<AnimationItem | null>(null)

const pet = computed(() => getPetById(props.petId))
const renderConfig = computed(() => pet.value.render ?? {})
const lottieStyle = computed<CSSProperties>(() => ({
  '--pet-scale': String(renderConfig.value.scale ?? 1),
  '--pet-offset-x': `${renderConfig.value.offsetX ?? 0}px`,
  '--pet-offset-y': `${renderConfig.value.offsetY ?? 0}px`,
}))

function mountAnimation() {
  if (!containerEl.value) return
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
    <div ref="containerEl" class="pet-lottie" :style="lottieStyle" />
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
