/** 右键菜单项定义 */
export interface MenuItemDef {
  key: string
  label: string
  emoji: string
}

/** 一个完整主题：7 个菜单项的标签+emoji */
export interface MenuTheme {
  id: string
  name: string
  desc: string
  items: MenuItemDef[]
}

/** 菜单项 key 列表，顺序决定菜单展示顺序 */
export const MENU_KEYS = ['tasks', 'review', 'plan', 'cost', 'chat', 'settings', 'update', 'quit'] as const
export type MenuKey = (typeof MENU_KEYS)[number]

/** 内置主题集合 */
export const MENU_THEMES: MenuTheme[] = [
  {
    id: 'default',
    name: '正经模式',
    desc: '规规矩矩的默认名',
    items: [
      { key: 'tasks', label: '任务提醒', emoji: '🔔' },
      { key: 'review', label: '今日复盘', emoji: '📋' },
      { key: 'plan', label: '今日计划', emoji: '📝' },
      { key: 'cost', label: '项目成本', emoji: '💰' },
      { key: 'chat', label: '聊天（大窗）', emoji: '💬' },
      { key: 'settings', label: '设置', emoji: '⚙️' },
      { key: 'update', label: '检查更新', emoji: '✨' },
      { key: 'quit', label: '退出', emoji: '🚪' },
    ],
  },
  {
    id: 'niuma',
    name: '牛马风',
    desc: '都是牛马，整点实在的',
    items: [
      { key: 'tasks', label: '催债清单', emoji: '🔔' },
      { key: 'review', label: '记工分', emoji: '📋' },
      { key: 'plan', label: '画饼', emoji: '📝' },
      { key: 'cost', label: '算账', emoji: '💰' },
      { key: 'chat', label: '摸鱼唠嗑', emoji: '💬' },
      { key: 'settings', label: '设置', emoji: '⚙️' },
      { key: 'update', label: '检查更新', emoji: '✨' },
      { key: 'quit', label: '跑路', emoji: '🚪' },
    ],
  },
  {
    id: 'minimal',
    name: '极简风',
    desc: '能少一个字就少一个字',
    items: [
      { key: 'tasks', label: '待办', emoji: '🔔' },
      { key: 'review', label: '工时', emoji: '📋' },
      { key: 'plan', label: '计划', emoji: '📝' },
      { key: 'cost', label: '成本', emoji: '💰' },
      { key: 'chat', label: '聊天', emoji: '💬' },
      { key: 'settings', label: '设置', emoji: '⚙️' },
      { key: 'update', label: '更新', emoji: '✨' },
      { key: 'quit', label: '退出', emoji: '🚪' },
    ],
  },
  {
    id: 'chuunibyou',
    name: '中二病',
    desc: '今天是拯救世界的一天',
    items: [
      { key: 'tasks', label: '任务副本', emoji: '🔔' },
      { key: 'review', label: '今日结算', emoji: '📋' },
      { key: 'plan', label: '作战部署', emoji: '📝' },
      { key: 'cost', label: '军费核算', emoji: '💰' },
      { key: 'chat', label: '通讯台', emoji: '💬' },
      { key: 'settings', label: '设置', emoji: '⚙️' },
      { key: 'update', label: '版本检视', emoji: '✨' },
      { key: 'quit', label: '登出', emoji: '🚪' },
    ],
  },
]

/** 缓存查找，按 id 取主题 */
export function getMenuTheme(id: string): MenuTheme {
  return MENU_THEMES.find(t => t.id === id) ?? MENU_THEMES[0]
}
