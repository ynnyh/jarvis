<script setup lang="ts">
// 左键单击小人时弹出的内容。
//
// 默认任务列表（保持原有行为）。用户可改成今日复盘 —— 这是用户的明确诉求：
// 不同人侧重不同，有人天天关心任务列表，有人下班前更关心今日已做了啥。
//
// 后续需要加更多动作（聊天大窗 / 风险分析 等）时，把选项追加到 select 里，
// App.vue::handleAvatarLeftClick 加 case 即可。

import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">左键点小人弹出</h3>
    <label class="settings-field">
      <span class="settings-field-label">动作</span>
      <select class="settings-input" v-model="store.config.leftClickAction">
        <option value="tasks">任务列表（默认）</option>
        <option value="review">今日复盘</option>
      </select>
    </label>
    <p class="settings-section-hint">右键菜单仍可访问其它面板，这里只改单击的默认动作</p>
  </section>
</template>
