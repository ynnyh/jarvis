<script setup lang="ts">
// 更新窗口：版本号、发布说明、下载进度、安装阶段一目了然。
//
// 之前只在气泡里塞一句「正在下载更新…」，用户看不到：
//   - 新版本是什么版本
//   - 下载到哪儿了
//   - 安装阶段还会不会自动重启
// 现在把这些都摊到一个独立窗口上，由 phase 状态机切换显示。
//
// 重入防护：所有动作按钮都通过 isBusy 禁用，避免点几次「下载更新」并发触发
// downloadAndInstall —— Tauri updater 插件本身不互斥，并发会让进度乱跳、
// 安装包也可能下两份。

import { computed } from 'vue'
import { useAppStore } from '../stores/app'
import type { useUpdater } from '../composables/useUpdater'

type UpdaterApi = ReturnType<typeof useUpdater>

const props = defineProps<{
  updater: UpdaterApi
}>()

const store = useAppStore()
const updater = props.updater

function formatMB(bytes: number): string {
  if (!bytes) return '0 MB'
  return (bytes / 1024 / 1024).toFixed(1) + ' MB'
}

const downloadedLabel = computed(() => {
  const { downloaded, total } = updater.downloadProgress.value
  if (total > 0) return `${formatMB(downloaded)} / ${formatMB(total)}`
  return formatMB(downloaded)
})

const phaseText = computed(() => {
  switch (updater.phase.value) {
    case 'idle': return '点击下方按钮检查更新'
    case 'checking': return '正在检查新版本…'
    case 'no-update': return '已是最新版本'
    case 'available': return `发现新版本，点击「下载并安装」开始更新`
    case 'downloading': return '正在下载更新包…'
    case 'installing': return '正在安装，请稍候（安装完会自动重启）'
    case 'installed': return '安装完成，即将重启 …'
    case 'error': return updater.lastError.value || '出错了'
    default: return ''
  }
})

const phaseClass = computed(() => `phase-${updater.phase.value}`)
const showProgress = computed(() =>
  updater.phase.value === 'downloading' || updater.phase.value === 'installing'
)
const showVersionPair = computed(() =>
  ['available', 'downloading', 'installing', 'installed'].includes(updater.phase.value)
)

async function handleCheck() {
  await updater.checkNow()
}

async function handleInstall() {
  await updater.installAndRestart()
}

function handleClose() {
  // busy 状态下别让用户手贱关掉 —— 关了等于没人看进度，但 install 还在跑
  if (updater.isBusy.value) return
  store.showUpdateWindow = false
  updater.reset()
}
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showUpdateWindow" class="update-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">✨</span>
          <span class="title-text">检查更新</span>
        </div>
        <button class="icon-btn" :disabled="updater.isBusy.value" :title="updater.isBusy.value ? '更新中，请等待完成' : '关闭'" @click="handleClose">×</button>
      </header>

      <div class="panel-body">
        <!-- 版本信息块 -->
        <div class="version-block">
          <div class="version-row">
            <span class="version-label">当前版本</span>
            <span class="version-value">v{{ updater.currentVersion.value || '—' }}</span>
          </div>
          <template v-if="showVersionPair">
            <div class="version-arrow">↓</div>
            <div class="version-row new">
              <span class="version-label">新版本</span>
              <span class="version-value">v{{ updater.newVersion.value }}</span>
            </div>
          </template>
        </div>

        <!-- 状态文字 -->
        <div class="phase-text" :class="phaseClass">
          <span v-if="updater.phase.value === 'checking' || updater.phase.value === 'downloading' || updater.phase.value === 'installing'" class="spinner" />
          {{ phaseText }}
        </div>

        <!-- 下载进度条 -->
        <div v-if="showProgress" class="progress-block">
          <div class="progress-bar">
            <div
              class="progress-fill"
              :class="{ indeterminate: updater.phase.value === 'installing' }"
              :style="{ width: updater.phase.value === 'installing' ? '100%' : updater.downloadProgress.value.percent + '%' }"
            />
          </div>
          <div class="progress-meta">
            <span v-if="updater.phase.value === 'downloading'">{{ downloadedLabel }}</span>
            <span v-if="updater.phase.value === 'downloading'">{{ updater.downloadProgress.value.percent }}%</span>
            <span v-else-if="updater.phase.value === 'installing'">安装中…</span>
          </div>
        </div>

        <!-- 发布说明 -->
        <div v-if="updater.releaseNotes.value && (updater.phase.value === 'available' || updater.phase.value === 'downloading')" class="notes-block">
          <div class="notes-title">更新内容</div>
          <pre class="notes-text">{{ updater.releaseNotes.value }}</pre>
        </div>
      </div>

      <footer class="panel-footer">
        <button
          v-if="updater.phase.value === 'available' || updater.phase.value === 'downloading' || updater.phase.value === 'installing'"
          class="primary-btn"
          :disabled="updater.isBusy.value"
          @click="handleInstall"
        >
          {{ updater.phase.value === 'available' ? '下载并安装' : (updater.phase.value === 'downloading' ? '下载中…' : '安装中…') }}
        </button>
        <button
          v-else
          class="primary-btn"
          :disabled="updater.isBusy.value"
          @click="handleCheck"
        >
          {{ updater.phase.value === 'checking' ? '检查中…' : '检查更新' }}
        </button>
        <button class="secondary-btn" :disabled="updater.isBusy.value" @click="handleClose">关闭</button>
      </footer>
    </div>
  </Transition>
</template>

<style scoped>
.update-panel {
  position: fixed;
  inset: var(--panel-top, 8px) var(--panel-right, 8px) var(--panel-bottom, 90px) var(--panel-left, 8px);
  display: flex;
  flex-direction: column;
  background: var(--panel-bg);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: var(--panel-border);
  border-radius: 14px;
  box-shadow: var(--panel-shadow);
  overflow: hidden;
  z-index: 58;
  color: var(--text);
}

.panel-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 8px 10px;
  background: var(--panel-header-bg);
  border-bottom: var(--panel-header-border);
}
.panel-title { display: flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 600; }
.title-icon { font-size: 14px; }

.icon-btn {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 16px; line-height: 1;
  color: var(--text-dim);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
}
.icon-btn:hover:not(:disabled) { color: var(--text); background: var(--surface-item-hover); }
.icon-btn:disabled { cursor: not-allowed; opacity: 0.35; }

.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.version-block {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 12px;
  background: var(--panel-header-bg);
  border: var(--divider);
  border-radius: 10px;
}
.version-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  font-size: 13px;
}
.version-label { color: var(--text-muted); font-size: 11px; }
.version-value { font-weight: 600; font-family: ui-monospace, SFMono-Regular, monospace; }
.version-row.new .version-value { color: var(--green-text); font-size: 16px; }
.version-arrow { color: var(--text-faint); font-size: 14px; line-height: 1; }

.phase-text {
  display: flex; align-items: center; gap: 6px; justify-content: center;
  padding: 6px 10px;
  font-size: 12px;
  text-align: center;
  border-radius: 8px;
  background: var(--panel-header-bg);
  color: var(--text-ghost);
}
.phase-text.phase-no-update { color: var(--text-dim); }
.phase-text.phase-available { color: var(--green-text); background: var(--green-bg); }
.phase-text.phase-error { color: var(--red-text); background: var(--red-bg); }
.phase-text.phase-installed { color: var(--green-text); }

.spinner {
  width: 10px; height: 10px;
  border: 1.5px solid rgba(255, 255, 255, 0.2);
  border-top-color: rgba(100, 200, 255, 0.9);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
  flex-shrink: 0;
}
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }

.progress-block {
  display: flex; flex-direction: column; gap: 4px;
}
.progress-bar {
  width: 100%; height: 8px;
  background: var(--surface-item-hover);
  border-radius: 4px;
  overflow: hidden;
}
.progress-fill {
  height: 100%;
  background: linear-gradient(90deg, rgba(0, 212, 255, 0.9), rgba(16, 185, 129, 0.9));
  border-radius: 4px;
  transition: width 0.2s ease;
}
.progress-fill.indeterminate {
  background: linear-gradient(90deg, rgba(167, 139, 250, 0.3), rgba(167, 139, 250, 0.9), rgba(167, 139, 250, 0.3));
  background-size: 200% 100%;
  animation: shimmer 1.2s linear infinite;
}
@keyframes shimmer {
  from { background-position: 200% 0; }
  to { background-position: -200% 0; }
}
.progress-meta {
  display: flex; justify-content: space-between;
  font-size: 10.5px;
  color: var(--text-dim);
  font-family: ui-monospace, SFMono-Regular, monospace;
}

.notes-block {
  display: flex; flex-direction: column; gap: 4px;
  padding: 10px;
  background: var(--panel-header-bg);
  border: 1px solid var(--border-soft);
  border-radius: 8px;
}
.notes-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--accent-text);
  letter-spacing: 0.5px;
}
.notes-text {
  margin: 0;
  font-size: 11.5px;
  line-height: 1.5;
  color: var(--text-ghost);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 140px;
  overflow-y: auto;
  font-family: inherit;
}

.panel-footer {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  background: var(--panel-header-bg);
  border-top: var(--divider);
}
.primary-btn {
  flex: 1;
  padding: 7px 10px;
  font-size: 12px;
  font-weight: 500;
  color: white;
  background: var(--btn-primary-bg);
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.primary-btn:hover:not(:disabled) {
  box-shadow: 0 4px 12px rgba(0, 212, 255, 0.3);
  transform: translateY(-1px);
}
.primary-btn:disabled {
  background: var(--surface-item-hover);
  color: var(--text-muted);
  cursor: not-allowed;
}
.secondary-btn {
  padding: 7px 14px;
  font-size: 12px;
  color: var(--text-ghost);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  cursor: pointer;
}
.secondary-btn:hover:not(:disabled) {
  color: var(--text);
  background: var(--surface-item-active);
}
.secondary-btn:disabled { opacity: 0.35; cursor: not-allowed; }

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
