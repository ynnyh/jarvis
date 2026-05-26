<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()

async function addRoot() {
  const picked = await invoke<string | null>('pick_directory', {
    title: '选择本地代码根目录（如 D:/coding）',
  })
  if (!picked) return
  if (store.config.repoRoots.includes(picked)) return
  store.config.repoRoots.push(picked)
}

function removeRoot(i: number) {
  store.config.repoRoots.splice(i, 1)
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">本地代码文件夹</h3>
    <p class="settings-section-hint">{{ store.config.assistantName }} 会扫描这些目录下的 git 仓库，关联到禅道任务以生成日报。每个目录第一层子文件夹的名字会被当作"业务线"</p>
    <ul class="settings-path-list">
      <li v-for="(p, i) in store.config.repoRoots" :key="i" class="settings-path-item">
        <span class="settings-path-text">{{ p }}</span>
        <button class="settings-path-remove" @click="removeRoot(i)" title="移除">×</button>
      </li>
      <li v-if="store.config.repoRoots.length === 0" class="settings-path-empty">还没有添加</li>
    </ul>
    <button class="settings-btn" @click="addRoot">+ 添加文件夹</button>
  </section>
</template>
