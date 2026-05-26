<script setup lang="ts">
// 排除的业务线（rootDir 下第一层目录名）。这些目录下的 commit 不进工时统计 / 日报。
// 存在 ~/.jarvis/excluded-business-lines.json，由 Rust 端 settings_extras 维护。

import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const lines = ref<string[]>([])
const newInput = ref('')

async function load() {
  try {
    lines.value = await invoke<string[]>('excluded_business_lines_load')
  } catch {
    lines.value = []
  }
}

async function save() {
  try {
    await invoke('excluded_business_lines_save', { lines: lines.value })
  } catch (e) {
    console.error('保存排除业务线失败:', e)
  }
}

function add() {
  const v = newInput.value.trim()
  if (!v) return
  if (lines.value.includes(v)) {
    newInput.value = ''
    return
  }
  lines.value.push(v)
  newInput.value = ''
  save()
}

function remove(i: number) {
  lines.value.splice(i, 1)
  save()
}

onMounted(load)
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">忽略的文件夹（业务线）</h3>
    <p class="settings-section-hint">这些业务线下的 commit 不会进入工时统计和日报。常用于个人项目、试验仓库等</p>
    <ul class="settings-path-list">
      <li v-for="(name, i) in lines" :key="name" class="settings-path-item">
        <span class="settings-path-text">{{ name }}</span>
        <button class="settings-path-remove" @click="remove(i)" title="移除">×</button>
      </li>
      <li v-if="lines.length === 0" class="settings-path-empty">没有忽略项</li>
    </ul>
    <div class="excl-add-row">
      <input class="settings-input excl-input" type="text"
        placeholder="业务线名（如 my-mcp-servers）"
        v-model="newInput"
        @keydown.enter="add" />
      <button class="settings-btn" @click="add">添加</button>
    </div>
  </section>
</template>

<style scoped>
.excl-add-row { display: flex; gap: 6px; margin-top: 6px; }
.excl-input { flex: 1; }
</style>
