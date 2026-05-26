<script setup lang="ts">
// 设置面板外壳：tab 导航 + 11 个 section 子组件挂载。
//
// 拆分背景：之前一个 .vue 800 多行什么都有，难维护。每个 section 独立成
// components/settings/*.vue，自带必要的状态和样式；这里只负责布局 + tab 切换。
// 共享样式集中在 components/settings/_settings-shared.css。
//
// Tab 设计：3 类
//   - 接入：禅道 / LLM / 代码目录 / 忽略业务线（数据源、外部依赖，初次配完很少改）
//   - 作息：工作日 / 时段 / 静默 / 仪式 / 小提示 / 临时覆盖（行为规则）
//   - 外观：助手名字
// activeTab 用 ref 保存在组件内，关掉面板下次开仍回默认（接入）。要持久化的话
// 移到 store；当前用户没要求，先不上。

import { computed, ref } from 'vue'
import { useConfigStore } from '../stores/config'

import AssistantNameSection from './settings/AssistantNameSection.vue'
import LeftClickActionSection from './settings/LeftClickActionSection.vue'
import ZentaoSection from './settings/ZentaoSection.vue'
import LlmSection from './settings/LlmSection.vue'
import RepoRootsSection from './settings/RepoRootsSection.vue'
import ExcludedLinesSection from './settings/ExcludedLinesSection.vue'
import WorkDaysSection from './settings/WorkDaysSection.vue'
import WorkPeriodsSection from './settings/WorkPeriodsSection.vue'
import QuietRulesSection from './settings/QuietRulesSection.vue'
import RitualsSection from './settings/RitualsSection.vue'
import WorkdayNudgesSection from './settings/WorkdayNudgesSection.vue'
import TodayOverrideSection from './settings/TodayOverrideSection.vue'

import './settings/_settings-shared.css'

const store = useConfigStore()

type TabKey = 'integrations' | 'rhythm' | 'general'
const TABS: Array<{ key: TabKey; label: string; icon: string }> = [
  { key: 'integrations', label: '接入', icon: '🔌' },
  { key: 'rhythm', label: '作息', icon: '⏱️' },
  { key: 'general', label: '外观', icon: '🎨' },
]
const activeTab = ref<TabKey>('integrations')

const phaseLabel = computed(() => {
  switch (store.phase) {
    case 'working': return '工作中'
    case 'lunch': return '午休'
    case 'before-work': return '尚未上班'
    case 'after-work': return '已下班'
    case 'weekend': return '周末'
    case 'dayoff': return '今天休假'
    case 'overtime': return '加班模式'
    default: return ''
  }
})
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showSettingsWindow" class="settings-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">⚙️</span>
          <span class="title-text">设置</span>
        </div>
        <button class="icon-btn" title="关闭" @click="store.showSettingsWindow = false">×</button>
      </header>

      <!-- 当前状态条 -->
      <div class="phase-bar" :class="`phase-${store.phase}`">
        <span class="phase-dot" />
        <span>当前：{{ phaseLabel }}</span>
        <span v-if="store.isQuietHours" class="phase-meta">静默中</span>
      </div>

      <!-- Tab 导航 -->
      <div class="tab-bar">
        <button
          v-for="t in TABS"
          :key="t.key"
          class="tab-btn"
          :class="{ active: activeTab === t.key }"
          @click="activeTab = t.key"
        >
          <span class="tab-icon">{{ t.icon }}</span>
          <span class="tab-label">{{ t.label }}</span>
        </button>
      </div>

      <div class="panel-body">
        <template v-if="activeTab === 'integrations'">
          <ZentaoSection />
          <LlmSection />
          <RepoRootsSection />
          <ExcludedLinesSection />
        </template>
        <template v-else-if="activeTab === 'rhythm'">
          <WorkDaysSection />
          <WorkPeriodsSection />
          <QuietRulesSection />
          <RitualsSection />
          <WorkdayNudgesSection />
          <TodayOverrideSection />
        </template>
        <template v-else>
          <AssistantNameSection />
          <LeftClickActionSection />
        </template>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.settings-panel {
  position: fixed;
  inset: 8px 8px 90px 8px;
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.97), rgba(15, 23, 42, 0.97));
  border: 1px solid rgba(100, 200, 255, 0.16);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  z-index: 60;
  color: rgba(255, 255, 255, 0.92);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.panel-title {
  display: flex; align-items: center; gap: 6px;
  font-size: 13px; font-weight: 600;
}
.title-icon { font-size: 14px; }
.icon-btn {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 16px; line-height: 1;
  color: rgba(255, 255, 255, 0.55);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
}
.icon-btn:hover { color: rgba(255, 255, 255, 0.95); background: rgba(255, 255, 255, 0.08); }

.phase-bar {
  display: flex; align-items: center; gap: 6px;
  padding: 4px 10px;
  font-size: 10px;
  background: rgba(0, 0, 0, 0.15);
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.65);
}
.phase-dot { width: 6px; height: 6px; border-radius: 50%; background: rgba(16, 185, 129, 0.9); }
.phase-working .phase-dot { background: rgba(16, 185, 129, 0.95); }
.phase-lunch .phase-dot { background: rgba(167, 139, 250, 0.95); }
.phase-after-work .phase-dot,
.phase-before-work .phase-dot { background: rgba(148, 163, 184, 0.7); }
.phase-weekend .phase-dot,
.phase-dayoff .phase-dot { background: rgba(245, 158, 11, 0.9); }
.phase-meta { margin-left: auto; color: rgba(245, 158, 11, 0.85); }

/* Tab 导航 */
.tab-bar {
  display: flex;
  gap: 2px;
  padding: 6px 6px 0;
  background: rgba(0, 0, 0, 0.1);
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
}
.tab-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 7px 8px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.55);
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.05);
  border-bottom: none;
  border-radius: 6px 6px 0 0;
  cursor: pointer;
  transition: all 0.15s;
}
.tab-btn:hover {
  color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.06);
}
.tab-btn.active {
  color: rgba(0, 212, 255, 0.95);
  background: rgba(0, 212, 255, 0.1);
  border-color: rgba(0, 212, 255, 0.3);
  border-bottom: 1px solid transparent;
}
.tab-icon { font-size: 13px; }
.tab-label { font-weight: 500; }

.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.panel-enter-active,
.panel-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.panel-enter-from,
.panel-leave-to {
  opacity: 0;
  transform: translateY(6px) scale(0.98);
}
</style>
