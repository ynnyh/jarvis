<script setup lang="ts">
// 插件管理（方案 C′5）：清单列表 + 启用开关 + 打开插件目录 + 重新加载。
// enabled 状态存 config.disabledPlugins（Rust plugin_set_enabled 落盘并走
// config-changed 广播，avatar 窗的插件引擎自动同步）；manifest 本身只读。
// invalid 插件红字展示原因，不影响其他插件。
import { onMounted, ref } from 'vue'
import {
  pluginList,
  pluginSetEnabled,
  pluginReload,
  pluginsDirOpen,
  type PluginInfo,
} from '../../api/plugins'
import ToggleSwitch from '../ui/ToggleSwitch.vue'

const rows = ref<PluginInfo[]>([])
const loading = ref(false)
const reloading = ref(false)
/** 正在切换开关的插件 id（防连点重复写配置） */
const toggling = ref('')
const tip = ref('')
const tipOk = ref(false)

function showTip(msg: string, ok: boolean) {
  tip.value = msg
  tipOk.value = ok
  setTimeout(() => { if (tip.value === msg) tip.value = '' }, 2000)
}

async function refresh() {
  loading.value = true
  try {
    rows.value = await pluginList()
  } catch (e) {
    console.error('plugin_list 失败:', e)
  } finally {
    loading.value = false
  }
}
onMounted(refresh)

async function onToggle(row: PluginInfo, enabled: boolean) {
  if (toggling.value) return
  toggling.value = row.id
  try {
    await pluginSetEnabled(row.id, enabled)
    row.enabled = enabled
  } catch (e) {
    console.error('plugin_set_enabled 失败:', e)
  } finally {
    toggling.value = ''
  }
}

function openDir() {
  pluginsDirOpen()
    .then(() => showTip('已打开插件目录（首次会放入喝水提醒示例）', true))
    .catch((e) => {
      console.error('plugins_dir_open 失败:', e)
      showTip(String(e), false)
    })
}

async function reloadAll() {
  if (reloading.value) return
  reloading.value = true
  try {
    await pluginReload()
    await refresh()
    showTip('已重新加载', true)
  } catch (e) {
    console.error('plugin_reload 失败:', e)
    showTip(String(e), false)
  } finally {
    reloading.value = false
  }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">插件</h3>
    <p class="settings-section-hint">
      声明式插件：触发器（cron / 每日时间窗 / 应用事件）命中后按清单执行动作。内置插件不可改，
      用户插件放在 ~/.jarvis/plugins/&lt;目录&gt;/plugin.json，改完点「重新加载」生效。
    </p>
    <div class="settings-actions">
      <button class="settings-btn" @click="openDir">打开插件目录</button>
      <button class="settings-btn" :disabled="reloading" @click="reloadAll">
        {{ reloading ? '重新加载中…' : '重新加载' }}
      </button>
    </div>
    <p v-if="tip" class="plugin-tip" :class="{ 'is-error': !tipOk }">{{ tip }}</p>

    <ul class="plugin-list">
      <li v-for="row in rows" :key="row.id" class="plugin-row">
        <div class="plugin-main">
          <div class="plugin-head">
            <span class="plugin-name">{{ row.name }}</span>
            <span class="plugin-badge" :class="row.source === 'builtin' ? 'is-builtin' : 'is-user'">
              {{ row.source === 'builtin' ? '内置' : '用户' }}
            </span>
            <span class="plugin-version">v{{ row.version }}</span>
          </div>
          <p v-if="row.description" class="plugin-desc">{{ row.description }}</p>
          <p v-if="row.triggerSummaries.length" class="plugin-triggers">
            {{ row.triggerSummaries.join(' · ') }}
          </p>
          <p v-if="row.invalidReason" class="plugin-invalid">{{ row.invalidReason }}</p>
        </div>
        <ToggleSwitch
          :model-value="row.enabled"
          :disabled="toggling === row.id"
          :label="row.enabled ? '已启用' : '已停用'"
          @update:model-value="(v: boolean) => onToggle(row, v)"
        />
      </li>
      <li v-if="!rows.length && !loading" class="plugin-empty">
        还没有插件。点「打开插件目录」从喝水提醒示例开始。
      </li>
    </ul>
  </section>
</template>

<style scoped>
.plugin-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 6px 0 0;
  padding: 0;
  list-style: none;
}
.plugin-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 10px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}
.plugin-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.plugin-head {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.plugin-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
}
.plugin-badge {
  padding: 1px 6px;
  font-size: 9.5px;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
}
.plugin-badge.is-builtin {
  color: var(--accent-text);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  border-color: color-mix(in srgb, var(--accent) 30%, transparent);
}
.plugin-badge.is-user {
  color: var(--green-text);
  background: color-mix(in srgb, var(--green) 12%, transparent);
  border-color: color-mix(in srgb, var(--green) 30%, transparent);
}
.plugin-version {
  font-size: 9.5px;
  color: var(--text-faint);
}
.plugin-desc {
  margin: 0;
  font-size: 10.5px;
  color: var(--text-dim);
  line-height: 1.5;
}
.plugin-triggers {
  margin: 0;
  font-size: 9.5px;
  color: var(--text-muted);
  word-break: break-all;
}
.plugin-invalid {
  margin: 0;
  font-size: 10.5px;
  color: var(--red-text);
  line-height: 1.5;
  word-break: break-all;
}
.plugin-empty {
  padding: 10px;
  font-size: 11px;
  color: var(--text-dim);
  background: var(--surface);
  border: 1px dashed var(--border);
  border-radius: var(--radius-md);
}
.plugin-tip {
  margin: 0;
  font-size: 10.5px;
  color: var(--green-text);
}
.plugin-tip.is-error {
  color: var(--red-text);
}
</style>
