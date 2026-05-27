<script setup lang="ts">
// 宠物形象选择：从 petManifest.ts 里取注册过的形象列表，用户下拉选。
//
// 切换后立刻生效（App.vue 里 PetAvatar 监听 petId 变化重载 Lottie）。
// 没必要重启进程，只是销毁旧动画加载新动画。

import { useConfigStore } from '../../stores/config'
import { PETS, PET_CATEGORY_LABELS } from '../../petManifest'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">宠物形象</h3>
    <label class="settings-field">
      <span class="settings-field-label">选什么</span>
      <select class="settings-input" v-model="store.config.petId">
        <option v-for="p in PETS" :key="p.id" :value="p.id">
          {{ p.name }} · {{ PET_CATEGORY_LABELS[p.category] }}
        </option>
      </select>
    </label>
    <p class="settings-section-hint">
      想加更多形象？把 Lottie .json 放到 <code>desktop/src/assets/pets/</code>，
      然后在 <code>petManifest.ts</code> 里注册一项。
    </p>
  </section>
</template>
