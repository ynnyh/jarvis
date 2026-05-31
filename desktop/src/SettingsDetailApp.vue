<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useConfigStore } from './stores/config'
import ErrorBoundary from './components/ErrorBoundary.vue'
import {
  SETTINGS_MENU,
  SETTINGS_PAGE_COMPONENTS,
  type SettingsPageKey,
} from './settings-menu'

import './components/settings/_settings-shared.css'

const store = useConfigStore()
const activePage = ref<SettingsPageKey>('channels')

const activeMeta = computed(() => SETTINGS_MENU.find(item => item.key === activePage.value) ?? SETTINGS_MENU[0])
const components = computed(() => SETTINGS_PAGE_COMPONENTS[activePage.value] ?? [])

function parsePage(value: unknown): SettingsPageKey {
  if (typeof value !== 'string') return 'channels'
  return SETTINGS_MENU.some(item => item.key === value) ? value as SettingsPageKey : 'channels'
}

async function loadPageFromUrl() {
  const params = new URLSearchParams(window.location.search)
  activePage.value = parsePage(params.get('page'))
}

async function closeWindow() {
  await invoke('settings_close')
}

let cleanupClose: (() => void) | null = null

onMounted(async () => {
  await store.load()
  await loadPageFromUrl()
  document.title = `${activeMeta.value.title} - 设置`
  const win = getCurrentWindow()
  cleanupClose = await win.onCloseRequested(async event => {
    event.preventDefault()
    await closeWindow()
  })
})

onUnmounted(() => {
  cleanupClose?.()
})
</script>

<template>
  <ErrorBoundary>
    <div class="detail-root">
      <header class="detail-header" data-tauri-drag-region>
        <div>
          <h1>{{ activeMeta.title }}</h1>
          <p>{{ activeMeta.desc }}</p>
        </div>
        <button class="close-btn" title="关闭" @click="closeWindow">×</button>
      </header>

      <main class="detail-body">
        <component
          :is="section"
          v-for="(section, index) in components"
          :key="index"
        />
      </main>
    </div>
  </ErrorBoundary>
</template>

<style scoped>
.detail-root {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: #0b1120;
  color: rgba(255, 255, 255, 0.92);
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
