<script setup lang="ts">
import { computed } from 'vue'
// ?raw 是 Vite 内置后缀，把目标文件作为字符串编进 bundle
// 路径四层向上：components/settings → components → src → desktop → 项目根
import changelogText from '../../../../CHANGELOG.md?raw'

interface Group { title: string; items: string[] }
interface Section { version: string; groups: Group[]; intro: string }

function parseSections(text: string): Section[] {
  const lines = text.split('\n')
  const sections: Section[] = []
  let current: Section | null = null
  let currentGroup: Group | null = null
  const introBuf: string[] = []

  const flushGroup = () => {
    if (current && currentGroup) {
      current.groups.push(currentGroup)
      currentGroup = null
    }
  }
  const flushSection = () => {
    flushGroup()
    if (current) {
      current.intro = introBuf.join('\n').trim()
      sections.push(current)
    }
    introBuf.length = 0
  }

  for (const line of lines) {
    const h2 = line.match(/^## (.+)$/)
    const h3 = line.match(/^### (.+)$/)
    if (h2) {
      flushSection()
      current = { version: h2[1].trim(), groups: [], intro: '' }
      continue
    }
    if (!current) continue
    if (h3) {
      flushGroup()
      currentGroup = { title: h3[1].trim(), items: [] }
      continue
    }
    const item = line.match(/^- (.+)$/)
    if (item && currentGroup) {
      currentGroup.items.push(item[1].trim())
      continue
    }
    if (!currentGroup) introBuf.push(line)
  }
  flushSection()
  return sections
}

const sections = computed(() => parseSections(changelogText))

// 极简内联 markdown：仅 **加粗**。先转义 HTML 避免 XSS，再放出 strong。
function renderInline(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">更新日志</h3>
    <p class="settings-section-hint">每个版本的变更记录，最新版本在最上面。</p>

    <div class="changelog-list">
      <article v-for="s in sections" :key="s.version" class="changelog-card">
        <header class="changelog-card-header">
          <span class="changelog-version">{{ s.version }}</span>
        </header>
        <p v-if="s.intro" class="changelog-intro">{{ s.intro }}</p>
        <div v-for="g in s.groups" :key="g.title" class="changelog-group">
          <h4 class="changelog-group-title">{{ g.title }}</h4>
          <ul class="changelog-items">
            <li v-for="(item, i) in g.items" :key="i" v-html="renderInline(item)" />
          </ul>
        </div>
      </article>
    </div>
  </section>
</template>

<style scoped>
.changelog-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 8px;
}
.changelog-card {
  padding: 14px 16px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
}
.changelog-card-header {
  margin-bottom: 8px;
}
.changelog-version {
  font-family: ui-monospace, monospace;
  font-size: 13px;
  font-weight: 600;
  color: rgba(147, 197, 253, 0.95);
}
.changelog-intro {
  margin: 0 0 10px;
  font-size: 12.5px;
  color: rgba(255, 255, 255, 0.6);
  line-height: 1.55;
  white-space: pre-wrap;
}
.changelog-group {
  margin-top: 10px;
}
.changelog-group:first-of-type {
  margin-top: 0;
}
.changelog-group-title {
  margin: 0 0 6px;
  font-size: 12px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.75);
}
.changelog-items {
  margin: 0;
  padding-left: 18px;
  list-style: disc;
}
.changelog-items li {
  margin-bottom: 4px;
  font-size: 12.5px;
  color: rgba(255, 255, 255, 0.85);
  line-height: 1.6;
}
.changelog-items :deep(strong) {
  color: rgba(255, 255, 255, 0.98);
  font-weight: 600;
}
</style>
