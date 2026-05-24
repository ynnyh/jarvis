<script setup lang="ts">
import type { Task, RiskAnalysis } from '../stores/app'
import { useConfigStore } from '../stores/config'

const configStore = useConfigStore()

const props = defineProps<{
  visible: boolean
  tasks: Task[]
  riskAnalysis: RiskAnalysis | null
}>()

const emit = defineEmits<{
  'update:visible': [value: boolean]
  'start-work': []
}>()

function close() {
  emit('update:visible', false)
}

function startWork() {
  emit('start-work')
}

const urgentTasks = props.tasks.filter(t => t.priority === 'urgent')
const overdueCount = props.riskAnalysis?.overdueTasks.length || 0
</script>

<template>
  <Transition name="panel">
    <div 
      v-if="visible"
      class="fixed left-40 top-10 w-80 bg-slate-900/95 backdrop-blur-xl rounded-2xl border border-white/10 shadow-2xl overflow-hidden z-40"
    >
      <!-- 头部 -->
      <div class="px-4 py-3 border-b border-white/10 flex items-center justify-between">
        <div class="flex items-center gap-2">
          <span class="text-xl">🤖</span>
          <span class="font-semibold">{{ configStore.config.assistantName }}</span>
        </div>
        <button @click="close" class="text-white/40 hover:text-white transition-colors">
          ✕
        </button>
      </div>

      <!-- 快捷操作 -->
      <div class="p-4 grid grid-cols-2 gap-2">
        <button 
          @click="startWork"
          class="flex items-center gap-2 px-3 py-2 bg-blue-600/20 hover:bg-blue-600/30 rounded-lg text-sm transition-colors"
        >
          <span>🚀</span>
          <span>开始工作</span>
        </button>
        <button 
          class="flex items-center gap-2 px-3 py-2 bg-amber-600/20 hover:bg-amber-600/30 rounded-lg text-sm transition-colors"
        >
          <span>📊</span>
          <span>生成日报</span>
        </button>
      </div>

      <!-- 今日任务 -->
      <div class="px-4 pb-4">
        <div class="flex items-center justify-between mb-3">
          <span class="text-sm font-medium text-slate-300">今日任务</span>
          <span class="text-xs text-slate-500">{{ tasks.length }} 个</span>
        </div>
        
        <div class="space-y-2">
          <div 
            v-for="task in tasks.slice(0, 3)" 
            :key="task.id"
            class="flex items-center gap-2 p-2 bg-white/5 rounded-lg"
          >
            <div 
              class="w-2 h-2 rounded-full"
              :class="{
                'bg-red-500': task.priority === 'urgent',
                'bg-yellow-500': task.priority === 'high',
                'bg-blue-500': task.priority === 'normal'
              }"
            />
            <div class="flex-1 min-w-0">
              <div class="text-sm truncate">{{ task.title }}</div>
              <div class="text-xs text-slate-500">{{ task.deadline }}</div>
            </div>
          </div>
          
          <div v-if="tasks.length === 0" class="text-center text-slate-500 text-sm py-4">
            暂无任务
          </div>
        </div>
      </div>

      <!-- 风险提示 -->
      <div v-if="overdueCount > 0" class="px-4 pb-4">
        <div class="bg-red-500/10 border border-red-500/20 rounded-lg p-3">
          <div class="flex items-center gap-2 text-red-400 text-sm">
            <span>⚠️</span>
            <span>{{ overdueCount }} 个风险任务</span>
          </div>
        </div>
      </div>

      <!-- 底部提示 -->
      <div class="px-4 py-3 bg-white/5 text-xs text-slate-500 text-center">
        点击 {{ configStore.config.assistantName }} 可以随时找我
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.panel-enter-active,
.panel-leave-active {
  transition: all 0.3s ease;
}

.panel-enter-from,
.panel-leave-to {
  opacity: 0;
  transform: translateX(-20px) scale(0.95);
}
</style>
