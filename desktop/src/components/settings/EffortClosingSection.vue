<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()

const lastWorkEnd = computed(() => {
  const ends = store.config.workSchedule.periods
    .map(period => period.end)
    .filter(Boolean)
    .sort()
  return ends[ends.length - 1] || '18:00'
})

const checkTimePreview = computed(() => {
  const [h, m] = lastWorkEnd.value.split(':').map(Number)
  const base = (Number.isFinite(h) ? h : 18) * 60 + (Number.isFinite(m) ? m : 0)
  const offset = Number(store.config.notifications.effortClosingMinutesAfterWork || 0)
  const total = Math.max(0, base + offset)
  const hh = Math.floor(total / 60) % 24
  const mm = total % 60
  return `${String(hh).padStart(2, '0')}:${String(mm).padStart(2, '0')}`
})

const telegramNotifyText = computed({
  get: () => store.config.channels.telegram.notifyChatIds.join('\n'),
  set: (value: string) => {
    store.config.channels.telegram.notifyChatIds = splitLines(value)
  },
})

const qqNotifyUserText = computed({
  get: () => store.config.channels.qqbot.notifyUserIds.join('\n'),
  set: (value: string) => {
    store.config.channels.qqbot.notifyUserIds = splitLines(value)
  },
})

const qqNotifyGroupText = computed({
  get: () => store.config.channels.qqbot.notifyGroupIds.join('\n'),
  set: (value: string) => {
    store.config.channels.qqbot.notifyGroupIds = splitLines(value)
  },
})

function splitLines(value: string): string[] {
  return value
    .split(/[\n,，]/)
    .map(v => v.trim())
    .filter(Boolean)
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">工时收尾提醒</h3>
    <p class="settings-section-hint">
      到下班后自动查询帆软工时。只在工作日生效；今日休假、周末或未配置工时统计姓名时不会提醒。
    </p>

    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.effortClosingCheck" />
      <span>下班后检查今日工时</span>
    </label>
    <label class="settings-toggle">
      <input
        type="checkbox"
        v-model="store.config.notifications.effortClosingChannelNotify"
        :disabled="!store.config.notifications.effortClosingCheck"
      />
      <span>工时不足时同步推送到已启用的 Telegram / QQ</span>
    </label>

    <div class="settings-grid">
      <label class="settings-field">
        <span class="settings-field-label">下班后多久检查</span>
        <input
          class="settings-input"
          type="number"
          min="0"
          max="180"
          step="5"
          v-model.number="store.config.notifications.effortClosingMinutesAfterWork"
          :disabled="!store.config.notifications.effortClosingCheck"
        />
      </label>
      <label class="settings-field">
        <span class="settings-field-label">目标工时</span>
        <input
          class="settings-input"
          type="number"
          min="0.5"
          max="24"
          step="0.5"
          v-model.number="store.config.notifications.effortClosingTargetHours"
          :disabled="!store.config.notifications.effortClosingCheck"
        />
      </label>
      <label class="settings-field">
        <span class="settings-field-label">重复提醒间隔</span>
        <input
          class="settings-input"
          type="number"
          min="0"
          max="240"
          step="15"
          v-model.number="store.config.notifications.effortClosingRepeatMinutes"
          :disabled="!store.config.notifications.effortClosingCheck"
        />
      </label>
      <label class="settings-field">
        <span class="settings-field-label">最晚提醒</span>
        <input
          class="settings-input"
          type="time"
          v-model="store.config.notifications.effortClosingLatestTime"
          :disabled="!store.config.notifications.effortClosingCheck"
        />
      </label>
    </div>

    <div class="settings-subsection">
      <h4 class="settings-subtitle">机器人提醒目标</h4>
      <p class="settings-section-hint">
        这里单独控制“工时不足提醒”推送给谁；留空时会沿用对应渠道的聊天白名单，方便兼容旧配置。
      </p>
      <div class="settings-grid">
        <label class="settings-field">
          <span class="settings-field-label">Telegram chat id</span>
          <textarea
            class="settings-input settings-textarea"
            v-model="telegramNotifyText"
            rows="3"
            placeholder="每行一个 chat id"
            :disabled="!store.config.notifications.effortClosingCheck || !store.config.notifications.effortClosingChannelNotify"
          />
        </label>
        <label class="settings-field">
          <span class="settings-field-label">QQ 单聊 openid</span>
          <textarea
            class="settings-input settings-textarea"
            v-model="qqNotifyUserText"
            rows="3"
            placeholder="每行一个 user_openid"
            :disabled="!store.config.notifications.effortClosingCheck || !store.config.notifications.effortClosingChannelNotify"
          />
        </label>
        <label class="settings-field">
          <span class="settings-field-label">QQ 群 openid</span>
          <textarea
            class="settings-input settings-textarea"
            v-model="qqNotifyGroupText"
            rows="3"
            placeholder="每行一个 group_openid"
            :disabled="!store.config.notifications.effortClosingCheck || !store.config.notifications.effortClosingChannelNotify"
          />
        </label>
      </div>
    </div>

    <div class="settings-info-list">
      <p>预计检查时间：{{ checkTimePreview }}</p>
      <p>重复提醒间隔填 0 表示只提醒一次。</p>
      <p>当前依赖“工时统计”里的帆软地址、账号、密码和中文姓名。</p>
      <p>机器人推送需要先启用并启动 Telegram / QQ 渠道；提醒目标为空时会沿用聊天白名单。</p>
    </div>
  </section>
</template>
