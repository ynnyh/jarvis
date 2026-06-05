<script setup lang="ts">
// 视觉风格选择器。卡片式取代下拉，交互更直观；每张卡用 3 色带预览该风格调色板。
// 切换即时落盘（config store 防抖）+ 经 config-changed 广播到所有窗口实时生效。
import { useConfigStore } from '../../stores/config'
import { STYLE_THEMES } from '../../style-themes'

const store = useConfigStore()

function pick(id: string) {
  store.config.styleTheme = id
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">视觉风格</h3>
    <div class="style-grid">
      <button
        v-for="t in STYLE_THEMES"
        :key="t.id"
        type="button"
        class="style-card"
        :class="{ active: store.config.styleTheme === t.id }"
        :aria-pressed="store.config.styleTheme === t.id"
        @click="pick(t.id)"
      >
        <span class="style-swatch">
          <span class="sw" :style="{ background: t.swatch[0] }" />
          <span class="sw" :style="{ background: t.swatch[1] }" />
          <span class="sw" :style="{ background: t.swatch[2] }" />
        </span>
        <span class="style-meta">
          <span class="style-name">{{ t.name }}</span>
          <span class="style-desc">{{ t.desc }}</span>
        </span>
        <span v-if="store.config.styleTheme === t.id" class="style-check">✓</span>
      </button>
    </div>
    <p class="settings-section-hint">
      切换后所有窗口实时生效，宠物形象不受影响。
    </p>
  </section>
</template>

<style scoped>
.style-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 8px;
  margin-top: 4px;
  max-width: 440px;
}
.style-card {
  position: relative;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px;
  text-align: left;
  font-family: inherit;
  background: var(--surface);
  border: var(--divider);
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}
.style-card:hover {
  background: var(--surface-item-hover);
  border-color: var(--border);
}
.style-card:focus-visible {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
}
.style-card.active {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
}

.style-swatch {
  display: inline-flex;
  flex: none;
  border-radius: 6px;
  overflow: hidden;
  border: var(--input-border);
}
.sw {
  display: block;
  width: 14px;
  height: 30px;
}

.style-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.style-name {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text);
}
.style-card.active .style-name {
  color: var(--accent-text);
}
.style-desc {
  font-size: 10px;
  color: var(--text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.style-check {
  position: absolute;
  top: 8px;
  right: 10px;
  font-size: 12px;
  font-weight: 700;
  color: var(--accent-text);
}
</style>
