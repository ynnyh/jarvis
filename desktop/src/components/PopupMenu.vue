<script setup lang="ts">
import { useAppStore } from '../stores/app'

const store = useAppStore()

// Emits
const emit = defineEmits<{
  startWork: []
}>()

function openTasks() {
  store.showMenu = false
  store.showTaskWindow = true
}

function openAnalyze() {
  store.showMenu = false
  store.showAnalyzeWindow = true
}

function generateReport() {
  store.addToast('日报生成', '今日日报已生成，共处理 3 个任务')
  store.showMenu = false
}

function startWork() {
  emit('startWork')
  store.showMenu = false
}
</script>

<template>
  <Transition name="menu">
    <div
      v-if="store.showMenu"
      class="absolute top-full left-1/2 -translate-x-1/2 mt-3 w-56 glass rounded-2xl py-2 shadow-2xl z-50"
    >
      <div class="px-3 py-2 text-xs text-gray-400 border-b border-white/10 mb-1">
        Jarvis 助手
      </div>

      <!-- 任务提醒入口 -->
      <button
        @click="openTasks"
        class="w-full px-4 py-2.5 text-left text-sm text-white hover:bg-white/10 transition-colors flex items-center gap-3"
      >
        <span class="text-lg">🔔</span>
        <span>任务提醒</span>
        <span
          v-if="store.overdueCount > 0"
          class="ml-auto text-xs bg-red-500/80 px-2 py-0.5 rounded-full"
        >
          {{ store.overdueCount }} 逾期
        </span>
        <span
          v-else-if="store.todayCount > 0"
          class="ml-auto text-xs bg-yellow-500/80 px-2 py-0.5 rounded-full"
        >
          {{ store.todayCount }} 待办
        </span>
      </button>

      <button
        @click="startWork"
        class="w-full px-4 py-2.5 text-left text-sm text-white hover:bg-white/10 transition-colors flex items-center gap-3"
      >
        <span class="text-lg">🚀</span>
        <span>开始今日工作</span>
      </button>

      <button
        @click="openAnalyze"
        class="w-full px-4 py-2.5 text-left text-sm text-white hover:bg-white/10 transition-colors flex items-center gap-3"
      >
        <span class="text-lg">🔍</span>
        <span>风险分析</span>
      </button>

      <button
        @click="generateReport"
        class="w-full px-4 py-2.5 text-left text-sm text-white hover:bg-white/10 transition-colors flex items-center gap-3"
      >
        <span class="text-lg">📊</span>
        <span>生成日报</span>
      </button>

      <div class="border-t border-white/10 mt-1 pt-1">
        <button
          @click="store.showMenu = false"
          class="w-full px-4 py-2.5 text-left text-sm text-gray-400 hover:bg-white/10 transition-colors"
        >
          关闭菜单
        </button>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.menu-enter-active,
.menu-leave-active {
  transition: all 0.2s ease;
}
.menu-enter-from,
.menu-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-8px) scale(0.95);
}
</style>
