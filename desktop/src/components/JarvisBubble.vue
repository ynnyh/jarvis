<script setup lang="ts">
defineProps<{
  message: string
  emotion: string
  state: string
}>()
</script>

<template>
  <div 
    class="absolute -top-20 left-1/2 -translate-x-1/2 mb-3 animate-fade-in"
    :class="{ 'animate-bounce': state === 'notifying' }"
  >
    <div class="relative bg-white/95 backdrop-blur-sm rounded-2xl px-4 py-3 shadow-2xl max-w-[280px]">
      <!-- 情绪图标 -->
      <span class="text-2xl absolute -left-2 -top-2">{{ emotion }}</span>
      
      <!-- 消息内容 -->
      <p class="text-slate-800 text-sm leading-relaxed pl-4">
        {{ message }}
      </p>
      
      <!-- 小三角 -->
      <div class="absolute -bottom-2 left-1/2 -translate-x-1/2 w-4 h-4 bg-white/95 rotate-45"></div>
    </div>
  </div>
</template>

<style scoped>
@keyframes fade-in {
  from {
    opacity: 0;
    transform: translateX(-50%) translateY(10px);
  }
  to {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
}

.animate-fade-in {
  animation: fade-in 0.3s ease-out;
}
</style>
