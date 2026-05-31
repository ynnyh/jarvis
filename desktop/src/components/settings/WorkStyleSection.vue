<script setup lang="ts">
import { useConfigStore, type WorkStyle } from '../../stores/config'

const store = useConfigStore()

const OPTIONS: Array<{ value: WorkStyle; title: string; desc: string }> = [
  { value: 'focused', title: '专注模式', desc: '任务比较集中，主要围绕少量固定项目持续推进。' },
  { value: 'multi', title: '并行模式', desc: '手上的项目和任务比较多，需要频繁切换上下文。' },
  { value: 'transactional', title: '事务模式', desc: '沟通、排障、部署、巡检这类工作占比更高。' },
  { value: 'balanced', title: '平衡模式', desc: '代码推进和事务处理都会有，整体比较均衡。' },
]
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">工作模式</h3>
    <div class="ws-list">
      <button
        v-for="opt in OPTIONS"
        :key="opt.value"
        type="button"
        class="ws-card"
        :class="{ active: store.config.workStyle === opt.value }"
        @click="store.config.workStyle = opt.value"
      >
        <span class="ws-radio" />
        <span class="ws-main">
          <strong>{{ opt.title }}</strong>
          <small>{{ opt.desc }}</small>
        </span>
      </button>
    </div>
    <p class="settings-section-hint">
      这个设置会影响今日计划的候选收敛、复盘时的任务推荐，以及没有提交时如何引导你补工时。
    </p>
  </section>
</template>

<style scoped>
.ws-list { display: flex; flex-direction: column; gap: 8px; }
.ws-card {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  width: 100%;
  padding: 10px 12px;
  text-align: left;
  color: rgba(255, 255, 255, 0.9);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 8px;
  cursor: pointer;
}
.ws-card:hover { background: rgba(14, 165, 233, 0.08); }
.ws-card.active { border-color: rgba(14, 165, 233, 0.55); background: rgba(14, 165, 233, 0.12); }
.ws-radio {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  margin-top: 2px;
  border-radius: 50%;
  border: 2px solid rgba(255, 255, 255, 0.3);
}
.ws-card.active .ws-radio {
  border-color: rgba(14, 165, 233, 0.9);
  background: radial-gradient(circle, rgba(14, 165, 233, 0.95) 0 4px, transparent 5px);
}
.ws-main { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
.ws-main strong { font-size: 13px; color: rgba(255, 255, 255, 0.96); }
.ws-main small { font-size: 11.5px; line-height: 1.4; color: rgba(255, 255, 255, 0.6); }
</style>
