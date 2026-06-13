<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '../../stores/config'
import { MENU_THEMES, getMenuTheme } from '../../menu-themes'
import CustomDropdown from '../ui/CustomDropdown.vue'
import type { DropdownOption } from '../ui/CustomDropdown.vue'

const store = useConfigStore()

const currentTheme = computed(() => getMenuTheme(store.config.menuTheme))

const themeOptions = computed<DropdownOption[]>(() =>
  MENU_THEMES.map(t => ({
    value: t.id,
    label: `${t.name} — ${t.desc}`,
  }))
)
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">右键菜单主题</h3>
    <label class="settings-field">
      <span class="settings-field-label">风格</span>
      <CustomDropdown
        v-model="store.config.menuTheme"
        :options="themeOptions"
      />
    </label>

    <div class="theme-preview">
      <div class="theme-preview-header">预览</div>
      <div class="theme-preview-group">
        <div class="theme-preview-group-label">日常</div>
        <div
          v-for="item in currentTheme.items.filter(i => ['tasks', 'review', 'plan'].includes(i.key))"
          :key="item.key"
          class="theme-preview-item"
        >
          <span>{{ item.emoji }}</span>
          <span>{{ item.label }}</span>
        </div>
      </div>
      <div class="theme-preview-group">
        <div class="theme-preview-group-label">系统</div>
        <div
          v-for="item in currentTheme.items.filter(i => ['chat', 'settings', 'update', 'quit'].includes(i.key))"
          :key="item.key"
          class="theme-preview-item"
        >
          <span>{{ item.emoji }}</span>
          <span>{{ item.label }}</span>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.theme-preview {
  margin-top: 10px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 8px;
  overflow: hidden;
  max-width: 280px;
}

.theme-preview-header {
  padding: 6px 10px;
  font-size: 10px;
  color: var(--text-dim);
  background: var(--surface-2);
  border-bottom: 1px solid rgba(148, 163, 184, 0.1);
}

.theme-preview-group {
  padding: 4px 0;
}

.theme-preview-group + .theme-preview-group {
  border-top: 1px solid rgba(148, 163, 184, 0.08);
}

.theme-preview-group-label {
  padding: 2px 10px;
  font-size: 9px;
  color: var(--text-faint);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.theme-preview-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  font-size: 12px;
  color: var(--text-ghost);
}
</style>
