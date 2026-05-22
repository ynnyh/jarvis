import { ref, onMounted, onUnmounted } from 'vue'

export function useDrag(elementRef: ReturnType<typeof ref<HTMLElement | null>>) {
  const isDragging = ref(false)
  const position = ref({ x: 100, y: 100 })
  const dragOffset = ref({ x: 0, y: 0 })

  function onMouseDown(e: MouseEvent) {
    if (!elementRef.value) return
    isDragging.value = true
    dragOffset.value = {
      x: e.clientX - position.value.x,
      y: e.clientY - position.value.y,
    }
  }

  function onMouseMove(e: MouseEvent) {
    if (!isDragging.value) return
    position.value = {
      x: e.clientX - dragOffset.value.x,
      y: e.clientY - dragOffset.value.y,
    }
  }

  function onMouseUp() {
    isDragging.value = false
  }

  onMounted(() => {
    const el = elementRef.value
    if (!el) return
    el.addEventListener('mousedown', onMouseDown)
    window.addEventListener('mousemove', onMouseMove)
    window.addEventListener('mouseup', onMouseUp)
  })

  onUnmounted(() => {
    const el = elementRef.value
    if (!el) return
    el.removeEventListener('mousedown', onMouseDown)
    window.removeEventListener('mousemove', onMouseMove)
    window.removeEventListener('mouseup', onMouseUp)
  })

  return { isDragging, position }
}
