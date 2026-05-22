<script setup lang="ts">
import { useAppStore } from '../stores/app'

const store = useAppStore()
</script>

<template>
  <div class="fixed top-4 right-4 z-[100] space-y-3 pointer-events-none">
    <TransitionGroup name="toast">
      <div
        v-for="toast in store.toasts"
        :key="toast.id"
        class="pointer-events-auto glass rounded-xl px-4 py-3 shadow-2xl min-w-[280px] max-w-[360px] border-l-4 border-l-jarvis-primary"
      >
        <div class="flex items-start justify-between">
          <div>
            <h4 class="text-white font-medium text-sm">{{ toast.title }}</h4>
            <p class="text-gray-400 text-xs mt-1">{{ toast.message }}</p>
          </div>
          <button
            @click="store.removeToast(toast.id)"
            class="text-gray-500 hover:text-white transition-colors ml-2 text-xs"
          >
            ✕
          </button>
        </div>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: all 0.3s ease;
}
.toast-enter-from {
  opacity: 0;
  transform: translateX(30px);
}
.toast-leave-to {
  opacity: 0;
  transform: translateX(30px);
}
</style>
