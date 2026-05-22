<script setup lang="ts">
import { useAppStore } from '../stores/app'

const store = useAppStore()

const priorityText: Record<string, string> = {
  urgent: '紧急',
  high: '高',
  normal: '中',
  low: '低',
}
</script>

<template>
  <Transition name="window">
    <div
      v-if="store.showAnalyzeWindow"
      class="fixed inset-0 flex items-center justify-center z-50"
      @click.self="store.showAnalyzeWindow = false"
    >
      <div class="glass rounded-2xl w-[520px] max-h-[640px] overflow-hidden shadow-2xl">
        <!-- 标题栏 -->
        <div class="flex items-center justify-between px-5 py-4 border-b border-white/10">
          <div class="flex items-center gap-2">
            <span class="text-xl">🔍</span>
            <h2 class="text-white font-semibold">AI 风险分析</h2>
          </div>
          <button
            @click="store.showAnalyzeWindow = false"
            class="text-gray-400 hover:text-white transition-colors text-lg"
          >
            ✕
          </button>
        </div>
        
        <!-- 分析内容 -->
        <div class="p-5 overflow-y-auto max-h-[520px]">
          <div v-if="store.loading" class="flex items-center justify-center py-16">
            <div class="w-8 h-8 border-2 border-jarvis-primary border-t-transparent rounded-full animate-spin" />
            <span class="ml-3 text-gray-400">AI 正在分析中...</span>
          </div>
          
          <div v-else-if="!store.riskAnalysis" class="text-center py-16 text-gray-400">
            <span class="text-4xl block mb-2">🤖</span>
            <p>暂无分析数据</p>
          </div>
          
          <div v-else class="space-y-5">
            <!-- 总结 -->
            <div class="glass-light rounded-xl p-4">
              <h3 class="text-jarvis-primary font-medium mb-2 flex items-center gap-2">
                <span>📊</span> 分析总结
              </h3>
              <p class="text-gray-300 text-sm leading-relaxed whitespace-pre-line">
                {{ store.riskAnalysis.summary }}
              </p>
            </div>
            
            <!-- 延期任务 -->
            <div v-if="store.riskAnalysis.overdueTasks.length > 0">
              <h3 class="text-red-400 font-medium mb-2 flex items-center gap-2 text-sm">
                <span>⚠️</span> 可能延期的任务 ({{ store.riskAnalysis.overdueTasks.length }})
              </h3>
              <div class="space-y-2">
                <div
                  v-for="task in store.riskAnalysis.overdueTasks"
                  :key="task.id"
                  class="glass-light rounded-lg p-3"
                >
                  <div class="flex items-center justify-between">
                    <span class="text-white text-sm">{{ task.title }}</span>
                    <span class="text-xs text-red-400">{{ task.deadline }}</span>
                  </div>
                  <div class="text-xs text-gray-500 mt-1">
                    状态: {{ task.status }} | 优先级: {{ priorityText[task.priority] }}
                  </div>
                </div>
              </div>
            </div>
            
            <!-- 高优先级 -->
            <div v-if="store.riskAnalysis.highPriorityTasks.length > 0">
              <h3 class="text-orange-400 font-medium mb-2 flex items-center gap-2 text-sm">
                <span>🔥</span> 高优先级任务 ({{ store.riskAnalysis.highPriorityTasks.length }})
              </h3>
              <div class="space-y-2">
                <div
                  v-for="task in store.riskAnalysis.highPriorityTasks"
                  :key="task.id"
                  class="glass-light rounded-lg p-3"
                >
                  <div class="flex items-center justify-between">
                    <span class="text-white text-sm">{{ task.title }}</span>
                    <span
                      class="text-xs px-2 py-0.5 rounded-full"
                      :class="{
                        'bg-red-500/20 text-red-400': task.priority === 'urgent',
                        'bg-orange-500/20 text-orange-400': task.priority === 'high',
                      }"
                    >
                      {{ priorityText[task.priority] }}
                    </span>
                  </div>
                </div>
              </div>
            </div>
            
            <!-- 依赖风险 -->
            <div v-if="store.riskAnalysis.dependencyRisks.length > 0">
              <h3 class="text-yellow-400 font-medium mb-2 flex items-center gap-2 text-sm">
                <span>🔗</span> 依赖风险 ({{ store.riskAnalysis.dependencyRisks.length }})
              </h3>
              <div class="space-y-2">
                <div
                  v-for="risk in store.riskAnalysis.dependencyRisks"
                  :key="risk.taskId"
                  class="glass-light rounded-lg p-3"
                >
                  <span class="text-white text-sm">{{ risk.taskTitle }}</span>
                  <p class="text-yellow-400/80 text-xs mt-1">{{ risk.reason }}</p>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.window-enter-active,
.window-leave-active {
  transition: all 0.3s ease;
}
.window-enter-from,
.window-leave-to {
  opacity: 0;
}
.window-enter-from > div,
.window-leave-to > div {
  transform: scale(0.95);
}
</style>
