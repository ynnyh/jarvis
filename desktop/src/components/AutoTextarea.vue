<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue'

// 高度跟随内容的 textarea：内容多高就撑多高，
// 收起时由外部 class 的 max-height 限高 + 内部滚动，展开时解除限高。
const props = withDefaults(defineProps<{
  modelValue: string
  disabled?: boolean
  placeholder?: string
  expanded?: boolean
}>(), { disabled: false, placeholder: '', expanded: false })

const emit = defineEmits<{ (e: 'update:modelValue', v: string): void }>()
const el = ref<HTMLTextAreaElement | null>(null)

function resize() {
  const ta = el.value
  if (!ta) return
  ta.style.height = 'auto'
  ta.style.height = `${ta.scrollHeight}px`
}

watch(() => [props.modelValue, props.expanded] as const, () => nextTick(resize))
onMounted(() => nextTick(resize))
</script>

<template>
  <textarea
    ref="el"
    :value="modelValue"
    :disabled="disabled"
    :placeholder="placeholder"
    @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value); resize()"
  ></textarea>
</template>
