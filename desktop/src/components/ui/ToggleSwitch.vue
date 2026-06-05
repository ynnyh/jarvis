<script setup lang="ts">
// 胶囊开关：替代原生 checkbox。无障碍 + 主题化（读 style.css 的设计 token）。
// 用法：<ToggleSwitch v-model="includeOvertime" label="含加班" />
//
// 可访问性：用 <button role="switch"> 承载，原生支持 Space/Enter 触发与 Tab 聚焦；
// 可见 label 文本即无障碍名；:focus-visible 给清晰焦点环。

const props = defineProps<{
  modelValue: boolean
  label?: string
  disabled?: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()

function toggle() {
  if (props.disabled) return
  emit('update:modelValue', !props.modelValue)
}
</script>

<template>
  <button
    type="button"
    role="switch"
    class="toggle"
    :class="{ 'is-on': modelValue, 'is-disabled': disabled }"
    :aria-checked="modelValue"
    :disabled="disabled"
    @click="toggle"
  >
    <span class="toggle-track">
      <span class="toggle-knob" />
    </span>
    <span v-if="label" class="toggle-label">{{ label }}</span>
  </button>
</template>

<style scoped>
.toggle {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 2px;
  font-family: inherit;
  font-size: 12px;
  color: var(--text-dim);
  background: transparent;
  border: none;
  cursor: pointer;
  user-select: none;
  -webkit-app-region: no-drag;
}
.toggle.is-disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.toggle-track {
  display: inline-flex;
  align-items: center;
  flex: none;
  width: 36px;
  height: 20px;
  padding: 2px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  transition:
    background var(--motion-fast) var(--ease),
    border-color var(--motion-fast) var(--ease),
    box-shadow var(--motion-fast) var(--ease);
}

.toggle-knob {
  width: 14px;
  height: 14px;
  border-radius: calc(var(--radius-control) - 3px);
  background: var(--text-dim);
  transition:
    transform var(--motion-base) var(--ease),
    background var(--motion-fast) var(--ease);
}

.toggle.is-on .toggle-track {
  background: color-mix(in srgb, var(--accent) 28%, transparent);
  border-color: var(--accent);
}
.toggle.is-on .toggle-knob {
  transform: translateX(16px);
  background: var(--accent);
}
.toggle.is-on .toggle-label {
  color: var(--text);
}

.toggle:hover:not(.is-disabled) .toggle-track {
  border-color: var(--accent);
}

.toggle:focus-visible {
  outline: none;
}
.toggle:focus-visible .toggle-track {
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent);
}

.toggle-label {
  white-space: nowrap;
  transition: color var(--motion-fast) var(--ease);
}
</style>
