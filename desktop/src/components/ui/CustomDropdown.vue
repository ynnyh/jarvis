<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, nextTick } from 'vue'

export interface DropdownOption {
  value: string
  label: string
  group?: string
  disabled?: boolean
}

const props = defineProps<{
  modelValue: string
  options: DropdownOption[]
  placeholder?: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const isOpen = ref(false)
const containerRef = ref<HTMLDivElement | null>(null)
const listRef = ref<HTMLDivElement | null>(null)

const selectedLabel = computed(() => {
  const opt = props.options.find(o => o.value === props.modelValue)
  return opt?.label ?? props.placeholder ?? '请选择'
})

// 按 group 分组
const groupedOptions = computed(() => {
  const groups: { group: string; items: DropdownOption[] }[] = []
  const map = new Map<string, DropdownOption[]>()

  for (const opt of props.options) {
    const g = opt.group ?? ''
    if (!map.has(g)) {
      map.set(g, [])
    }
    map.get(g)!.push(opt)
  }

  for (const [group, items] of map) {
    groups.push({ group, items })
  }

  return groups
})

function toggle() {
  isOpen.value = !isOpen.value
  if (isOpen.value) {
    nextTick(() => scrollToSelected())
  }
}

function select(value: string) {
  emit('update:modelValue', value)
  isOpen.value = false
}

function scrollToSelected() {
  if (!listRef.value) return
  const selected = listRef.value.querySelector('.dropdown-option.selected')
  if (selected) {
    selected.scrollIntoView({ block: 'nearest' })
  }
}

function onClickOutside(e: MouseEvent) {
  if (containerRef.value && !containerRef.value.contains(e.target as Node)) {
    isOpen.value = false
  }
}

onMounted(() => {
  document.addEventListener('click', onClickOutside)
})

onBeforeUnmount(() => {
  document.removeEventListener('click', onClickOutside)
})
</script>

<template>
  <div ref="containerRef" class="custom-dropdown" :class="{ open: isOpen }">
    <button
      type="button"
      class="dropdown-trigger"
      @click="toggle"
    >
      <span class="dropdown-label">{{ selectedLabel }}</span>
      <span class="dropdown-arrow">▾</span>
    </button>
    <div v-show="isOpen" ref="listRef" class="dropdown-list">
      <template v-for="group in groupedOptions" :key="group.group">
        <div v-if="group.group" class="dropdown-group-label">{{ group.group }}</div>
        <button
          v-for="opt in group.items"
          :key="opt.value"
          type="button"
          class="dropdown-option"
          :class="{ selected: opt.value === modelValue, disabled: opt.disabled }"
          :disabled="opt.disabled"
          @click="select(opt.value)"
        >
          {{ opt.label }}
        </button>
      </template>
    </div>
  </div>
</template>

<style scoped>
.custom-dropdown {
  position: relative;
  width: 100%;
}

.dropdown-trigger {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  font-family: inherit;
  font-size: 13px;
  color: var(--text);
  background: var(--input-bg);
  border: 1px solid var(--input-border);
  border-radius: 6px;
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.dropdown-trigger:hover {
  border-color: var(--border);
}
.dropdown-trigger:focus,
.custom-dropdown.open .dropdown-trigger {
  outline: none;
  border-color: var(--input-focus-border);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 15%, transparent);
}

.dropdown-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: left;
  flex: 1;
}

.dropdown-arrow {
  flex-shrink: 0;
  margin-left: 8px;
  font-size: 10px;
  color: var(--text-dim);
  transition: transform 0.15s;
}
.custom-dropdown.open .dropdown-arrow {
  transform: rotate(180deg);
}

.dropdown-list {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  max-height: 280px;
  overflow-y: auto;
  background: var(--popup-bg);
  border: 1px solid var(--panel-border);
  border-radius: 8px;
  box-shadow: var(--panel-shadow);
  z-index: 100;
  padding: 4px;
}

.dropdown-group-label {
  padding: 6px 8px 4px;
  font-size: 10px;
  font-weight: 600;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.dropdown-group-label:not(:first-child) {
  margin-top: 4px;
  border-top: 1px solid var(--divider);
  padding-top: 8px;
}

.dropdown-option {
  display: block;
  width: 100%;
  padding: 8px 10px;
  font-family: inherit;
  font-size: 12px;
  color: var(--text);
  background: transparent;
  border: none;
  border-radius: 4px;
  text-align: left;
  cursor: pointer;
  transition: background 0.1s;
}
.dropdown-option:hover:not(.disabled) {
  background: var(--surface-item-hover);
}
.dropdown-option.selected {
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent-text);
}
.dropdown-option.disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* 滚动条 */
.dropdown-list::-webkit-scrollbar {
  width: 4px;
}
.dropdown-list::-webkit-scrollbar-track {
  background: transparent;
}
.dropdown-list::-webkit-scrollbar-thumb {
  background: var(--text-ghost);
  border-radius: 2px;
}
</style>
