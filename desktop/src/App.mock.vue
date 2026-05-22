<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAppStore } from './stores/app'
import { useNotificationStore } from './stores/notification'
import { useDrag } from './composables/useDrag'
import { usePolling } from './composables/usePolling'
import { ToolRegistry } from './composables/useToolRegistry.mock'
import Avatar from './components/Avatar.vue'
import PopupMenu from './components/PopupMenu.vue'
import TaskWindow from './components/TaskWindow.vue'
import AnalyzeWindow from './components/AnalyzeWindow.vue'
import ToastContainer from './components/ToastContainer.vue'

const store = useAppStore()
const notificationStore = useNotificationStore()
const avatarWrapper = ref<HTMLElement | null>(null)
const { position } = useDrag(avatarWrapper)

const isLoading = ref(false)
const agentState = ref<'idle' | 'thinking' | 'working' | 'notifying' | 'error'>('idle')

// 从 Mock 加载数据
async function loadTasks() {
  isLoading.value = true
  agentState.value = 'thinking'
  
  try {
    // 获取今日任务
    const todayTasks = await ToolRegistry.getTodayTasks()
    store.todayTasks = todayTasks

    // 获取风险分析
    const riskAnalysis = await ToolRegistry.analyzeRisk()
    if (riskAnalysis) {
      store.riskAnalysis = riskAnalysis
    }

    // 获取 Agent 状态
    const state = await ToolRegistry.getAgentState()
    agentState.value = state.state as any

  } catch (error) {
    console.error('加载任务失败:', error)
  } finally {
    isLoading.value = false
    agentState.value = 'idle'
  }
}

// 轮询检查
async function checkAndNotify() {
  await loadTasks()

  const urgentCount = store.todayTasks.filter(t => t.priority === 'urgent').length
  if (urgentCount > 0) {
    const notification = {
      id: Date.now().toString(),
      title: '⚠️ 紧急任务提醒',
      body: `今天有 ${urgentCount} 个紧急任务需要处理！`,
      priority: 'urgent' as const,
      type: 'task' as const,
      timestamp: Date.now(),
      read: false,
    }
    notificationStore.add(notification)
    store.addToast(notification.title, notification.body)
  }

  if (store.riskAnalysis && store.riskAnalysis.overdueTasks.length > 0) {
    const notification = {
      id: (Date.now() + 1).toString(),
      title: '🔥 风险预警',
      body: `发现 ${store.riskAnalysis.overdueTasks.length} 个任务已延期或即将延期`,
      priority: 'urgent' as const,
      type: 'risk' as const,
      timestamp: Date.now(),
      read: false,
    }
    notificationStore.add(notification)
    store.addToast(notification.title, notification.body)
  }
}

// 执行 Action：开始今日工作
async function startTodayWork() {
  store.addToast('🚀 开始今日工作', '正在分析任务风险...')
  agentState.value = 'working'
  
  try {
    const result = await ToolRegistry.startTodayWork()
    if (result.success) {
      store.addToast('✅ 准备工作完成', '已获取今日任务和风险分析')
      await loadTasks()
      agentState.value = 'notifying'
      setTimeout(() => { agentState.value = 'idle' }, 3000)
    } else {
      store.addToast('❌ 准备工作失败', result.error || '未知错误')
      agentState.value = 'error'
    }
  } catch (error) {
    store.addToast('❌ 调用失败', String(error))
    agentState.value = 'error'
  }
}

// 启动调度器
async function startScheduler() {
  try {
    await ToolRegistry.startScheduler()
    store.addToast('⏰ 调度器已启动', '将自动检查任务风险')
  } catch (error) {
    console.error('启动调度器失败:', error)
  }
}

usePolling(checkAndNotify, 60000) // 测试时用 1 分钟

onMounted(async () => {
  await loadTasks()
  
  // 启动时延迟显示欢迎
  setTimeout(() => {
    store.addToast('🤖 Jarvis 已启动', '正在监控您的任务状态...')
  }, 1500)

  // 延迟启动调度器
  setTimeout(() => {
    startScheduler()
  }, 3000)
})
</script>

<template>
  <div class="w-full h-full relative bg-gradient-to-br from-slate-900 via-purple-900 to-slate-900">
    <!-- 桌宠 -->
    <div
      ref="avatarWrapper"
      class="absolute"
      :style="{ left: position.x + 'px', top: position.y + 'px' }"
    >
      <Avatar :state="agentState" />
      <PopupMenu @start-work="startTodayWork" />
    </div>

    <!-- 弹窗 -->
    <TaskWindow />
    <AnalyzeWindow />

    <!-- 气泡通知 -->
    <ToastContainer />

    <!-- 加载指示器 -->
    <div
      v-if="isLoading"
      class="fixed bottom-4 right-4 px-3 py-1.5 bg-black/50 backdrop-blur-sm rounded-full text-white text-xs"
    >
      加载中...
    </div>

    <!-- 状态面板 -->
    <div class="fixed top-4 left-4 glass rounded-xl p-4 text-white text-sm max-w-xs">
      <h3 class="font-bold mb-2">🤖 Jarvis 状态</h3>
      <div class="space-y-1 text-xs opacity-80">
        <p>状态: <span class="text-cyan-400">{{ agentState }}</span></p>
        <p>今日任务: <span class="text-yellow-400">{{ store.todayTasks.length }}</span></p>
        <p>风险任务: <span class="text-red-400">{{ store.riskAnalysis?.overdueTasks.length || 0 }}</span></p>
      </div>
    </div>

    <!-- 操作提示 -->
    <div class="fixed bottom-4 left-4 text-white/50 text-xs">
      点击桌宠打开菜单 | 拖拽移动位置
    </div>
  </div>
</template>

<style>
/* 全局样式 */
body {
  margin: 0;
  overflow: hidden;
}

.glass {
  background: rgba(255, 255, 255, 0.1);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.2);
}
</style>
