<script setup lang="ts">
import { computed } from 'vue'
import { marked, Renderer } from 'marked'
import hljs from 'highlight.js/lib/core'
import typescript from 'highlight.js/lib/languages/typescript'
import javascript from 'highlight.js/lib/languages/javascript'
import rust from 'highlight.js/lib/languages/rust'
import python from 'highlight.js/lib/languages/python'
import go from 'highlight.js/lib/languages/go'
import bash from 'highlight.js/lib/languages/bash'
import json from 'highlight.js/lib/languages/json'
import sql from 'highlight.js/lib/languages/sql'
import xml from 'highlight.js/lib/languages/xml'
import yaml from 'highlight.js/lib/languages/yaml'
import diff from 'highlight.js/lib/languages/diff'
import plaintext from 'highlight.js/lib/languages/plaintext'

hljs.registerLanguage('typescript', typescript)
hljs.registerLanguage('javascript', javascript)
hljs.registerLanguage('rust', rust)
hljs.registerLanguage('python', python)
hljs.registerLanguage('go', go)
hljs.registerLanguage('bash', bash)
hljs.registerLanguage('sh', bash)
hljs.registerLanguage('shell', bash)
hljs.registerLanguage('json', json)
hljs.registerLanguage('sql', sql)
hljs.registerLanguage('html', xml)
hljs.registerLanguage('xml', xml)
hljs.registerLanguage('yaml', yaml)
hljs.registerLanguage('yml', yaml)
hljs.registerLanguage('diff', diff)
hljs.registerLanguage('text', plaintext)
hljs.registerLanguage('plaintext', plaintext)

const renderer = new Renderer()
renderer.code = ({ text, lang }) => {
  const validLang = lang && hljs.getLanguage(lang) ? lang : 'plaintext'
  let highlighted: string
  try {
    highlighted = hljs.highlight(text, { language: validLang }).value
  } catch {
    highlighted = text
  }
  return `<div class="code-block-wrapper">
    <div class="code-lang">${lang ? escapeHtml(lang) : 'text'}</div>
    <pre class="code-block"><code>${highlighted}</code></pre>
  </div>`
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

marked.setOptions({
  renderer,
  gfm: true,
  breaks: true,
})

const props = defineProps<{
  content: string
}>()

const html = computed(() => {
  try {
    return marked.parse(props.content) as string
  } catch {
    return escapeHtml(props.content)
  }
})
</script>

<template>
  <div class="markdown-body" v-html="html" />
</template>

<style>
/* Markdown 基础样式 — 全局（非 scoped），因为 v-html 渲染的内容不在作用域内 */
.markdown-body {
  line-height: 1.65;
  word-break: break-word;
  overflow-wrap: break-word;
}
.markdown-body > *:first-child { margin-top: 0; }
.markdown-body > *:last-child { margin-bottom: 0; }

.markdown-body p {
  margin: 6px 0;
}
.markdown-body p:first-child { margin-top: 0; }

.markdown-body h1,
.markdown-body h2,
.markdown-body h3,
.markdown-body h4 {
  margin: 14px 0 6px;
  font-weight: 600;
  color: var(--text);
}
.markdown-body h1 { font-size: 16px; }
.markdown-body h2 { font-size: 15px; }
.markdown-body h3 { font-size: 14px; }
.markdown-body h4 { font-size: 13px; }

.markdown-body ul,
.markdown-body ol {
  padding-left: 20px;
  margin: 6px 0;
}
.markdown-body li { margin: 2px 0; }
.markdown-body li > p { margin: 0; }

.markdown-body blockquote {
  margin: 8px 0;
  padding: 4px 12px;
  border-left: 3px solid var(--accent-border);
  color: var(--text-dim);
  background: var(--surface);
  border-radius: 0 4px 4px 0;
}

.markdown-body a {
  color: var(--accent-2);
  text-decoration: none;
}
.markdown-body a:hover { text-decoration: underline; }

.markdown-body strong { color: var(--text); font-weight: 600; }

.markdown-body hr {
  border: none;
  border-top: 1px solid var(--border);
  margin: 12px 0;
}

.markdown-body table {
  border-collapse: collapse;
  margin: 8px 0;
  font-size: 12px;
  width: 100%;
}
.markdown-body th,
.markdown-body td {
  border: 1px solid var(--border);
  padding: 5px 8px;
  text-align: left;
}
.markdown-body th {
  background: var(--surface);
  font-weight: 600;
  color: var(--text);
}
.markdown-body td { color: var(--text); }
.markdown-body tr:nth-child(even) td { background: var(--surface-2); }

/* 代码块 */
.code-block-wrapper {
  margin: 8px 0;
  border-radius: 6px;
  overflow: hidden;
  border: 1px solid rgba(255, 255, 255, 0.08);
}
.code-lang {
  padding: 3px 10px;
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.5);
  background: rgba(0, 0, 0, 0.35);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  user-select: none;
}
.code-block {
  margin: 0;
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.45);
  overflow-x: auto;
  font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.5;
  color: rgba(255, 255, 255, 0.9);
}
.code-block code {
  background: none;
  padding: 0;
  font-size: inherit;
}

/* 行内代码 */
.markdown-body code:not(.code-block code) {
  padding: 1px 5px;
  background: var(--surface-2);
  border-radius: 4px;
  font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace;
  font-size: 0.9em;
  color: var(--accent-text);
}

/* highlight.js 暗色主题覆盖 */
.code-block .hljs { color: rgba(255, 255, 255, 0.9); background: transparent; }
.code-block .hljs-keyword { color: rgba(198, 120, 221, 0.95); }
.code-block .hljs-string { color: rgba(152, 195, 121, 0.95); }
.code-block .hljs-number { color: rgba(209, 154, 102, 0.95); }
.code-block .hljs-comment { color: rgba(93, 108, 132, 0.95); font-style: italic; }
.code-block .hljs-built_in { color: rgba(86, 182, 194, 0.95); }
.code-block .hljs-title { color: rgba(224, 175, 104, 0.95); }
.code-block .hljs-title.class_ { color: rgba(224, 175, 104, 0.95); }
.code-block .hljs-title.function_ { color: rgba(97, 175, 239, 0.95); }
.code-block .hljs-attr { color: rgba(156, 220, 254, 0.95); }
.code-block .hljs-params { color: rgba(255, 255, 255, 0.8); }
.code-block .hljs-literal { color: rgba(86, 182, 194, 0.95); }
.code-block .hljs-meta { color: rgba(139, 185, 234, 0.9); }
.code-block .hljs-selector-class { color: rgba(224, 175, 104, 0.95); }
.code-block .hljs-selector-tag { color: rgba(198, 120, 221, 0.95); }
.code-block .hljs-tag { color: rgba(86, 182, 194, 0.95); }
.code-block .hljs-name { color: rgba(86, 182, 194, 0.95); }
.code-block .hljs-attribute { color: rgba(224, 175, 104, 0.95); }
.code-block .hljs-variable { color: rgba(255, 255, 255, 0.85); }
.code-block .hljs-symbol { color: rgba(209, 154, 102, 0.95); }
.code-block .hljs-section { color: rgba(224, 175, 104, 0.95); }
.code-block .hljs-type { color: rgba(86, 182, 194, 0.95); }
.code-block .hljs-regexp { color: rgba(152, 195, 121, 0.95); }
.code-block .hljs-deletion { color: rgba(239, 83, 80, 0.9); }
.code-block .hljs-addition { color: rgba(152, 195, 121, 0.9); }
</style>
