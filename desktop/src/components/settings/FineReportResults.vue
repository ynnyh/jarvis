<template src="./FineReportResults.template.html"></template>
<script setup lang="ts">
/* eslint-disable @typescript-eslint/no-unused-vars -- template 引用外部 .html 文件，ESLint 无法检测 template 内的变量使用 */
import { computed, ref } from 'vue'

interface EffortRecord {
  date: string
  department: string
  employee: string
  dailyTotalHours: number
  itemHours: number
  projectName: string
  system: string
  taskName: string
  workContent: string
}

interface EffortAppendixItem {
  [key: string]: string | number
}

interface EffortTheme {
  name: string
  hours: number
  task_count: number
  project_count: number
  systems: string[]
  tasks: string[]
  work_items: string[]
}

interface EffortReportResponse {
  mode: 'effort' | 'report'
  begin: string
  end: string
  range: string
  summaryText: string
  themes: EffortTheme[]
  appendix: {
    begin: string
    end: string
    total_hours: number
    task_count: number
    project_count: number
    system_count: number
    task_hours: EffortAppendixItem[]
    project_hours: EffortAppendixItem[]
    daily_hours: EffortAppendixItem[]
  }
  records: EffortRecord[]
}

interface DailyGroup {
  date: string
  hours: number
  count: number
}

interface AbnormalDay {
  date: string
  hours: number
  weekday: string
  holiday: string | null
  isWorkday: boolean
  type: 'low' | 'overtime'
}

const props = defineProps<{
  queryMode: 'effort' | 'report' | null
  effortRecords: EffortRecord[]
  reportResult: EffortReportResponse | null
  showEffortDetail: boolean
  expandSummary: boolean
  expandThemes: boolean
  expandProjects: boolean
  expandTasks: boolean
}>()

const emit = defineEmits<{
  'update:showEffortDetail': [value: boolean]
  'update:expandSummary': [value: boolean]
  'update:expandThemes': [value: boolean]
  'update:expandProjects': [value: boolean]
  'update:expandTasks': [value: boolean]
  'copy-text': [text: string, successText: string]
}>()

const FULL_DAY_HOURS = 8
const PREVIEW_COUNT = 3

const totalItemHours = computed(() =>
  props.effortRecords.reduce((sum, item) => sum + (item.itemHours || 0), 0),
)

const effortDailyGroups = computed<DailyGroup[]>(() => {
  const map = new Map<string, { hours: number; count: number }>()
  for (const r of props.effortRecords) {
    const d = r.date || '未知日期'
    const entry = map.get(d) || { hours: 0, count: 0 }
    entry.hours += r.itemHours || 0
    entry.count += 1
    map.set(d, entry)
  }
  return Array.from(map.entries())
    .map(([date, { hours, count }]) => ({ date, hours, count }))
    .sort((a, b) => a.date.localeCompare(b.date))
})

const effortWorkDays = computed(() => {
  const days = effortDailyGroups.value.length
  return days > 1
})

const dailyHours = computed(() => props.reportResult?.appendix.daily_hours ?? [])

const abnormalDays = computed<AbnormalDay[]>(() => {
  const all = dailyHours.value
  if (!all.length) return []
  return all
    .filter(item => {
      const h = Number(item.hours || 0)
      const wd = Boolean(item.isWorkday)
      return (wd && h < FULL_DAY_HOURS) || (!wd && h > 0)
    })
    .map(item => {
      const hours = Number(item.hours || 0)
      const isWorkday = Boolean(item.isWorkday)
      return {
        date: String(item.date),
        hours,
        weekday: String(item.weekday || ''),
        holiday: (item.holiday as string) || null,
        isWorkday,
        type: isWorkday ? 'low' as const : 'overtime' as const,
      }
    })
})

const topProjects = computed(() => props.reportResult?.appendix.project_hours ?? [])
const topTasks = computed(() => props.reportResult?.appendix.task_hours ?? [])

const visibleThemes = computed(() => {
  const all = props.reportResult?.themes ?? []
  return props.expandThemes ? all : all.slice(0, PREVIEW_COUNT)
})

const visibleProjects = computed(() => {
  const all = topProjects.value
  return props.expandProjects ? all : all.slice(0, PREVIEW_COUNT)
})

const visibleTasks = computed(() => {
  const all = topTasks.value
  return props.expandTasks ? all : all.slice(0, PREVIEW_COUNT)
})

function handleToggleDetail(value: boolean) {
  emit('update:showEffortDetail', value)
}

function handleCopyText(text: string, successText: string) {
  emit('copy-text', text, successText)
}
</script>
<style src="./FineReportResults.style.css" scoped></style>
