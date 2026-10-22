<script setup lang="ts">
// 宠物「主动搭话」开关（方案 B2）。
// 开启：早晨计划/晚间复盘/时段问候/逾期提醒节点，宠物先用固定文案开口，
// LLM 在 ≤5s 内生成一句有上下文的人话后原位替换。
// 关闭：这些节点照常提醒，但只显示固定文案，完全不发 LLM 请求（零网络）。
import { useConfigStore } from '../../stores/config'
import ToggleSwitch from '../ui/ToggleSwitch.vue'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">主动搭话</h3>
    <ToggleSwitch
      v-model="store.config.petProactiveChat"
      label="在定计划、晚间复盘、时段问候、逾期提醒时，让宠物用一句自己的话开口"
    />
    <p class="settings-section-hint">
      关闭后提醒照常弹出，但只显示固定文案，不再调用 AI 模型
    </p>
  </section>
</template>
