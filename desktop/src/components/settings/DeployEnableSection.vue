<script setup lang="ts">
import { useConfigStore } from '../../stores/config'
import ToggleSwitch from '../ui/ToggleSwitch.vue'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">对话式发版</h3>
    <ToggleSwitch v-model="store.config.deployEnabled" label="启用对话式发版（Jenkins）" />
    <p class="settings-section-hint">
      发版属高危操作，默认关闭。开启后在「接入」里出现「对话式发版」配置项。
    </p>
  </section>
</template>
