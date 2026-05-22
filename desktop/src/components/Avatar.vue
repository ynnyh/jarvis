<script setup lang="ts">
import { ref, computed } from 'vue'
import { useAppStore } from '../stores/app'

const store = useAppStore()
const avatarRef = ref<HTMLElement | null>(null)

// Props: 接收 Agent 状态
const props = defineProps<{
  state?: 'idle' | 'thinking' | 'working' | 'notifying' | 'error'
}>()

defineExpose({ avatarRef })

// 根据状态计算样式
const stateClass = computed(() => {
  switch (props.state) {
    case 'thinking':
      return 'animate-pulse-glow-blue'
    case 'working':
      return 'animate-pulse-glow-green'
    case 'notifying':
      return 'animate-pulse-glow-yellow'
    case 'error':
      return 'animate-pulse-glow-red'
    default:
      return 'animate-pulse-glow'
  }
})

const stateColor = computed(() => {
  switch (props.state) {
    case 'thinking':
      return '#3b82f6' // blue-500
    case 'working':
      return '#10b981' // emerald-500
    case 'notifying':
      return '#f59e0b' // amber-500
    case 'error':
      return '#ef4444' // red-500
    default:
      return '#00d4ff' // cyan
  }
})

const statusDotClass = computed(() => {
  switch (props.state) {
    case 'thinking':
      return 'bg-blue-400 animate-pulse'
    case 'working':
      return 'bg-emerald-400 animate-pulse'
    case 'notifying':
      return 'bg-amber-400 animate-bounce'
    case 'error':
      return 'bg-red-400'
    default:
      return 'bg-emerald-400'
  }
})
</script>

<template>
  <div
    ref="avatarRef"
    class="relative w-[120px] h-[120px] cursor-pointer select-none"
    @click="store.showMenu = !store.showMenu"
  >
    <!-- 外圈光晕 - 根据状态变色 -->
    <div 
      class="absolute inset-0 rounded-full transition-all duration-500"
      :class="stateClass"
      :style="{ '--glow-color': stateColor }"
    />
    
    <!-- 主体 -->
    <div class="absolute inset-2 rounded-full glass flex items-center justify-center animate-float">
      <!-- 机器人脸 -->
      <svg viewBox="0 0 100 100" class="w-16 h-16">
        <!-- 头部 -->
        <rect x="20" y="15" width="60" height="50" rx="12" fill="none" :stroke="stateColor" stroke-width="2.5" />
        <!-- 天线 -->
        <line x1="50" y1="15" x2="50" y2="5" :stroke="stateColor" stroke-width="2" />
        <circle cx="50" cy="3" r="3" :fill="stateColor" class="animate-pulse" />
        <!-- 左眼 -->
        <rect x="30" y="32" width="14" height="10" rx="3" :fill="stateColor" :class="{ 'animate-pulse': state === 'thinking' || state === 'working' }" />
        <!-- 右眼 -->
        <rect x="56" y="32" width="14" height="10" rx="3" :fill="stateColor" :class="{ 'animate-pulse': state === 'thinking' || state === 'working' }" />
        <!-- 嘴巴 -->
        <rect x="38" y="52" width="24" height="4" rx="2" :fill="stateColor" opacity="0.7" />
        <!-- 身体 -->
        <rect x="32" y="68" width="36" height="20" rx="6" fill="none" :stroke="stateColor" stroke-width="2" opacity="0.5" />
        <!-- 胸口灯 -->
        <circle cx="50" cy="76" r="4" :fill="stateColor" :class="{ 'animate-pulse': state === 'working' }" />
      </svg>
    </div>
    
    <!-- 状态指示点 -->
    <div 
      class="absolute bottom-2 right-2 w-3 h-3 rounded-full transition-colors duration-300"
      :class="statusDotClass"
    />

    <!-- 状态文字提示 -->
    <div
      v-if="state && state !== 'idle'"
      class="absolute -top-8 left-1/2 -translate-x-1/2 px-2 py-1 bg-black/70 rounded text-xs text-white whitespace-nowrap"
    >
      {{ state === 'thinking' ? '思考中...' : state === 'working' ? '工作中...' : state === 'notifying' ? '新通知' : state === 'error' ? '出错了' : '' }}
    </div>
  </div>
</template>

<style scoped>
/* 不同状态的光晕动画 */
@keyframes pulse-glow-blue {
  0%, 100% { box-shadow: 0 0 20px rgba(59, 130, 246, 0.4), 0 0 40px rgba(59, 130, 246, 0.2); }
  50% { box-shadow: 0 0 30px rgba(59, 130, 246, 0.6), 0 0 60px rgba(59, 130, 246, 0.3); }
}

@keyframes pulse-glow-green {
  0%, 100% { box-shadow: 0 0 20px rgba(16, 185, 129, 0.4), 0 0 40px rgba(16, 185, 129, 0.2); }
  50% { box-shadow: 0 0 30px rgba(16, 185, 129, 0.6), 0 0 60px rgba(16, 185, 129, 0.3); }
}

@keyframes pulse-glow-yellow {
  0%, 100% { box-shadow: 0 0 20px rgba(245, 158, 11, 0.4), 0 0 40px rgba(245, 158, 11, 0.2); }
  50% { box-shadow: 0 0 30px rgba(245, 158, 11, 0.6), 0 0 60px rgba(245, 158, 11, 0.3); }
}

@keyframes pulse-glow-red {
  0%, 100% { box-shadow: 0 0 20px rgba(239, 68, 68, 0.4), 0 0 40px rgba(239, 68, 68, 0.2); }
  50% { box-shadow: 0 0 30px rgba(239, 68, 68, 0.6), 0 0 60px rgba(239, 68, 68, 0.3); }
}

.animate-pulse-glow-blue {
  animation: pulse-glow-blue 2s ease-in-out infinite;
}

.animate-pulse-glow-green {
  animation: pulse-glow-green 2s ease-in-out infinite;
}

.animate-pulse-glow-yellow {
  animation: pulse-glow-yellow 1.5s ease-in-out infinite;
}

.animate-pulse-glow-red {
  animation: pulse-glow-red 1s ease-in-out infinite;
}
</style>
