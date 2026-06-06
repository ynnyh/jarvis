<script setup lang="ts">
import { computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../stores/config'
import { SETTINGS_MENU, type SettingsMenuItem, type SettingsPageKey } from '../settings-menu'

import './settings/_settings-shared.css'

const store = useConfigStore()

const groupedMenu = computed(() => {
  // 「对话式发版」属高危功能，未开启时不在菜单里出现（响应式，开关一开即时显示）
  const menu = SETTINGS_MENU.filter(item => item.key !== 'deploy' || store.config.deployEnabled)
  const groups: Array<{ name: string; items: SettingsMenuItem[] }> = []
  for (const item of menu) {
    let group = groups.find(g => g.name === item.group)
    if (!group) {
      group = { name: item.group, items: [] }
      groups.push(group)
    }
    group.items.push(item)
  }
  return groups
})

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

const aiStatus = computed(() => {
  const profiles = store.config.llmProfiles ?? []
  const activeProfile = profiles.find(p => p.id === store.config.activeLlmProfileId)
  if (activeProfile) {
    return activeProfile.name?.trim() || activeProfile.model || '已配置'
  }
  if (profiles.length > 0) {
    return `已配 ${profiles.length} 个`
  }
  return store.config.llm.apiKey ? store.config.llm.model : '未配置'
})

function pageStatus(key: SettingsPageKey) {
  if (key === 'general') return store.config.autoStartOnBoot ? '自启已开' : '自启已关'
  if (key === 'zentao') return store.config.zentao.account ? '已配置' : '未配置'
  if (key === 'ai') return aiStatus.value
  if (key === 'channels') {
    const names = []
    if (store.config.channels.telegram.enabled) names.push('Telegram')
    if (store.config.channels.qqbot.enabled) names.push('QQ')
    return names.join(' / ') || '未启用'
  }
  if (key === 'code') return `${store.config.repoRoots.length} 个目录`
  if (key === 'dailyNudges') return store.config.notifications.todayPlanPromptEnabled ? '已启用' : '已配置'
  if (key === 'effortClosing') {
    return store.config.notifications.effortClosingCheck ? '已启用' : '已关闭'
  }
  if (key === 'personalization') return store.config.assistantName || '已配置'
  return store.config.assistantName
}

function closeSettingsPanel() {
  store.showSettingsWindow = false
}

function openSettingsDetail(page: SettingsPageKey) {
  store.showSettingsWindow = false
  invoke('settings_open', { page }).catch(error => console.error('settings_open failed:', error))
}

function openChatWindow() {
  closeSettingsPanel()
  invoke('chat_open').catch(error => console.error('chat_open failed:', error))
}
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showSettingsWindow" class="settings-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-text">设置</span>
          <span class="title-sub">菜单</span>
        </div>
        <button class="icon-btn" title="关闭" @click="closeSettingsPanel">×</button>
      </header>

      <div class="phase-bar" :class="`phase-${store.phase}`">
        <span class="phase-dot" />
        <span>当前：{{ phaseLabel }}</span>
        <span v-if="store.isQuietHours" class="phase-meta">静默中</span>
      </div>

      <div class="menu-body">
        <section class="menu-group">
          <h3>常用</h3>
          <div
            role="button"
            tabindex="0"
            class="menu-item menu-item-primary"
            @click="openChatWindow"
            @keydown.enter="openChatWindow"
            @keydown.space.prevent="openChatWindow"
          >
            <span class="menu-main">
              <strong>聊天大窗</strong>
              <small>打开完整对话窗口</small>
            </span>
            <span class="menu-status">打开</span>
            <span class="menu-arrow">›</span>
          </div>
        </section>

        <section v-for="group in groupedMenu" :key="group.name" class="menu-group">
          <h3>{{ group.name }}</h3>
          <div
            v-for="item in group.items"
            :key="item.key"
            role="button"
            tabindex="0"
            class="menu-item"
            @click="openSettingsDetail(item.key)"
            @keydown.enter="openSettingsDetail(item.key)"
            @keydown.space.prevent="openSettingsDetail(item.key)"
          >
            <span class="menu-main">
              <strong>{{ item.title }}</strong>
              <small>{{ item.desc }}</small>
            </span>
            <span class="menu-status">{{ pageStatus(item.key) }}</span>
            <span class="menu-arrow">›</span>
          </div>
        </section>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.settings-panel {
  position: fixed;
  inset: var(--panel-top, 8px) var(--panel-right, 8px) var(--panel-bottom, 90px) var(--panel-left, 8px);
  display: flex;
  flex-direction: column;
  background: var(--popup-bg);
  border: var(--panel-border);
  border-radius: 12px;
  box-shadow: var(--panel-shadow);
  overflow: hidden;
  z-index: 60;
  color: var(--text);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 11px;
  background: var(--panel-header-bg);
  border-bottom: var(--divider);
}

.panel-title {
  display: flex;
  align-items: baseline;
  gap: 7px;
}

.title-text {
  font-size: 13px;
  font-weight: 650;
}

.title-sub {
  font-size: 10px;
  color: var(--text-muted);
}

.icon-btn {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 18px;
  line-height: 1;
  color: var(--text-dim);
  background: transparent;
  border: none;
  border-radius: 6px;
  cursor: pointer;
}

.icon-btn:hover {
  color: var(--text);
  background: var(--surface-item-hover);
}

.phase-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 11px;
  font-size: 10px;
  background: var(--surface);
  border-bottom: var(--divider-soft);
  color: var(--text-dim);
}

.phase-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--green-text);
}

.phase-lunch .phase-dot { background: var(--blue-text); }
.phase-after-work .phase-dot,
.phase-before-work .phase-dot { background: var(--text-dim); }
.phase-weekend .phase-dot,
.phase-dayoff .phase-dot { background: var(--yellow-text); }

.phase-meta {
  margin-left: auto;
  color: var(--yellow-text);
}

.menu-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.menu-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.menu-group h3 {
  margin: 0;
  font-size: 10px;
  line-height: 1;
  font-weight: 700;
  color: var(--accent-text);
}

.menu-item {
  display: grid;
  box-sizing: border-box;
  width: 100%;
  grid-template-columns: minmax(0, 1fr) minmax(44px, auto) 12px;
  align-items: center;
  gap: 7px;
  min-height: 50px;
  padding: 8px 10px;
  color: var(--text);
  background-color: var(--surface);
  border: var(--panel-border);
  border-radius: 8px;
  cursor: pointer;
  text-align: left;
  user-select: none;
  outline: none;
}

.menu-item:hover,
.menu-item:focus-visible {
  background-color: var(--surface-item-hover);
  border-color: var(--accent-border);
  box-shadow: var(--shadow-1);
}

.menu-item-primary {
  background-color: var(--accent-glow);
  border-color: var(--accent-border);
}

.menu-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.menu-main strong {
  display: block;
  font-size: 13px;
  line-height: 1.2;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.menu-main small {
  display: block;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 10px;
  color: var(--text-muted);
}

.menu-status {
  justify-self: end;
  max-width: 82px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 10px;
  line-height: 1;
  padding: 4px 0;
  color: var(--green-text-light);
}

.menu-arrow {
  justify-self: end;
  font-size: 20px;
  line-height: 1;
  color: var(--text-faint);
}

.panel-enter-active,
.panel-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.panel-enter-from,
.panel-leave-to {
  opacity: 0;
  transform: translateY(6px) scale(0.98);
}
</style>
