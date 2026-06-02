<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useConfigStore } from './stores/config'
import ErrorBoundary from './components/ErrorBoundary.vue'
import {
  SETTINGS_MENU,
  SETTINGS_PAGE_COMPONENTS,
  LEGACY_PAGE_MAP,
  type SettingsPageKey,
} from './settings-menu'

import './components/settings/_settings-shared.css'

const store = useConfigStore()
const activePage = ref<SettingsPageKey>('channels')
const collapsedGroups = ref<Set<string>>(new Set())

const groupedMenu = computed(() => {
  const groups: { name: string; order: number; items: typeof SETTINGS_MENU }[] = []
  const seen = new Set<string>()
  for (const item of SETTINGS_MENU) {
    if (!seen.has(item.group)) {
      seen.add(item.group)
      groups.push({ name: item.group, order: item.groupOrder, items: [] })
    }
  }
  for (const g of groups) {
    g.items.push(...SETTINGS_MENU.filter(i => i.group === g.name))
  }
  groups.sort((a, b) => a.order - b.order)
  return groups
})

const activeMeta = computed(() => SETTINGS_MENU.find(item => item.key === activePage.value) ?? SETTINGS_MENU[0])
const components = computed(() => SETTINGS_PAGE_COMPONENTS[activePage.value] ?? [])

function resolveKey(raw: string): SettingsPageKey {
  const legacy = LEGACY_PAGE_MAP[raw]
  if (legacy) return legacy
  if (SETTINGS_MENU.some(item => item.key === raw)) return raw as SettingsPageKey
  return 'channels'
}

function parsePage(value: unknown): SettingsPageKey {
  if (typeof value === 'string') return resolveKey(value)
  return 'channels'
}

function navigateTo(key: SettingsPageKey) {
  activePage.value = key
  document.title = `${SETTINGS_MENU.find(i => i.key === key)?.title ?? '设置'} - 设置`
}

function toggleGroup(name: string) {
  if (collapsedGroups.value.has(name)) {
    collapsedGroups.value.delete(name)
  } else {
    collapsedGroups.value.add(name)
  }
  collapsedGroups.value = new Set(collapsedGroups.value)
}

async function loadPageFromUrl() {
  const params = new URLSearchParams(window.location.search)
  activePage.value = parsePage(params.get('page'))
  document.title = `${activeMeta.value.title} - 设置`
}

async function closeWindow() {
  await invoke('settings_close')
}

let cleanupClose: (() => void) | null = null
let cleanupPageChanged: (() => void) | null = null

onMounted(async () => {
  await store.load()
  await loadPageFromUrl()
  window.addEventListener('settings-page-changed', loadPageFromUrl)
  cleanupPageChanged = () => window.removeEventListener('settings-page-changed', loadPageFromUrl)
  const win = getCurrentWindow()
  cleanupClose = await win.onCloseRequested(async event => {
    event.preventDefault()
    await closeWindow()
  })
})

onUnmounted(() => {
  cleanupClose?.()
  cleanupPageChanged?.()
})
</script>

<template>
  <ErrorBoundary>
    <div class="settings-root">
      <!-- 侧边栏 -->
      <aside class="settings-sidebar">
        <div class="sidebar-brand">设置</div>
        <nav class="sidebar-nav">
          <div v-for="g in groupedMenu" :key="g.name" class="sidebar-group">
            <button class="sidebar-group-title" @click="toggleGroup(g.name)">
              <span>{{ collapsedGroups.has(g.name) ? '▸' : '▾' }}</span>
              <span>{{ g.name }}</span>
            </button>
            <div v-if="!collapsedGroups.has(g.name)" class="sidebar-items">
              <button
                v-for="item in g.items"
                :key="item.key"
                class="sidebar-item"
                :class="{ active: activePage === item.key }"
                @click="navigateTo(item.key)"
              >
                <span class="sidebar-item-title">{{ item.title }}</span>
                <span class="sidebar-item-desc">{{ item.desc }}</span>
              </button>
            </div>
          </div>
        </nav>
      </aside>

      <!-- 内容区 -->
      <div class="settings-main">
        <header class="detail-header" data-tauri-drag-region>
          <div>
            <h1>{{ activeMeta.title }}</h1>
            <p>{{ activeMeta.desc }}</p>
          </div>
          <button class="close-btn" title="关闭" @click="closeWindow">x</button>
        </header>

        <div class="detail-body">
          <component
            :is="section"
            v-for="(section, index) in components"
            :key="index"
          />
        </div>
      </div>
    </div>
  </ErrorBoundary>
</template>

<style scoped>
.settings-root {
  width: 100%;
  height: 100vh;
  display: flex;
  background: #0b1120;
  color: rgba(255, 255, 255, 0.92);
  font-family: system-ui, -apple-system, sans-serif;
  overflow: hidden;
}

/* 侧边栏 */
.settings-sidebar {
  flex: none;
  width: 190px;
  display: flex;
  flex-direction: column;
  background: rgba(17, 24, 39, 0.98);
  border-right: 1px solid rgba(148, 163, 184, 0.12);
  user-select: none;
  overflow-y: auto;
}

.sidebar-brand {
  padding: 18px 16px 14px;
  font-size: 15px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.85);
  letter-spacing: .03em;
}

.sidebar-nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 0 8px 12px;
}

.sidebar-group {
  margin-bottom: 4px;
}

.sidebar-group-title {
  width: 100%;
  padding: 6px 8px 4px;
  font-size: 11px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.4);
  text-transform: uppercase;
  letter-spacing: .06em;
  background: transparent;
  border: none;
  text-align: left;
  cursor: pointer;
  display: flex;
  gap: 4px;
  align-items: center;
}

.sidebar-group-title:hover {
  color: rgba(255, 255, 255, 0.6);
}

.sidebar-items {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.sidebar-item {
  width: 100%;
  padding: 7px 10px;
  border-radius: 6px;
  border: none;
  background: transparent;
  text-align: left;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.sidebar-item:hover {
  background: rgba(255, 255, 255, 0.06);
}

.sidebar-item.active {
  background: rgba(59, 130, 246, 0.15);
}

.sidebar-item-title {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.9);
  font-weight: 500;
}

.sidebar-item.active .sidebar-item-title {
  color: rgba(147, 197, 253, 0.95);
  font-weight: 600;
}

.sidebar-item-desc {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.35);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 右侧主内容区 */
.settings-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.detail-header {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 22px 14px;
  background: rgba(17, 24, 39, 0.98);
  border-bottom: 1px solid rgba(148, 163, 184, 0.18);
  user-select: none;
}

.detail-header h1 {
  margin: 0;
  font-size: 19px;
  line-height: 1.25;
  font-weight: 700;
}

.detail-header p {
  margin: 5px 0 0;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.46);
}

.close-btn {
  flex: none;
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 18px;
  line-height: 1;
  color: rgba(255, 255, 255, 0.6);
  background: transparent;
  border: none;
  border-radius: 6px;
  cursor: pointer;
}

.close-btn:hover {
  color: rgba(255, 255, 255, 0.95);
  background: rgba(255, 255, 255, 0.08);
}

.detail-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 18px 22px 24px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
</style>
