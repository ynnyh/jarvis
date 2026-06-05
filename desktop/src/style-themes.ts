// 视觉风格主题注册表（与 menu-themes.ts 同构）。
//
// 每个风格 = 一套设计 token（定义在 style.css 的 :root[data-theme="<id>"]）+ 元信息。
//
// 新增风格：
//   1. 在 style.css 复制一段 :root[data-theme="<id>"] 改 token 值
//   2. 在下方 STYLE_THEMES 加一项（swatch 用于设置里选择器的预览色点）

export interface StyleTheme {
  /** 对应 style.css 里 [data-theme="<id>"] 与 config.styleTheme */
  id: string
  name: string
  desc: string
  /** 选择器卡片的预览色点：[底色, 主强调, 次强调] */
  swatch: [string, string, string]
}

export const STYLE_THEMES: StyleTheme[] = [
  {
    id: 'sci-fi',
    name: '霓虹风',
    desc: '品红霓虹 · 深紫暗夜',
    swatch: ['#0d0b1a', '#ff2d95', '#00d4ff'],
  },
  {
    id: 'playful',
    name: '俏皮风',
    desc: '奶油薄荷 · 圆润弹跳',
    swatch: ['#fff0f0', '#ff5c8a', '#ff8f1c'],
  },
  {
    id: 'zen',
    name: '治愈风',
    desc: '大地米色 · 柔和纸感',
    swatch: ['#f5f3ec', '#a8b09e', '#d4cdbc'],
  },
  {
    id: 'minimal',
    name: '极简风',
    desc: '灰阶单色 · 干净商务',
    swatch: ['#0f172a', '#e2e8f0', '#94a3b8'],
  },
  {
    id: 'matrix',
    name: '矩阵风',
    desc: '黑客帝国 · 数字雨',
    swatch: ['#0a0e0a', '#00ff41', '#00d4aa'],
  },
  {
    id: 'cyber',
    name: '未来风',
    desc: '全息战士 · 颗粒朦胧',
    swatch: ['#050a14', '#00d4ff', '#a855f7'],
  },
]

export const DEFAULT_STYLE_THEME = 'sci-fi'

export function getStyleTheme(id: string): StyleTheme {
  return STYLE_THEMES.find(t => t.id === id) ?? STYLE_THEMES[0]
}
