import { watch } from 'vue'
import { useConfigStore } from '../stores/config'
import { getStyleTheme } from '../style-themes'

/**
 * 把当前 config.styleTheme 应用到 <html data-theme>，并监听其变化实时切换。
 * 在需要主题化的窗口根组件 setup() 里调用一次即可。
 *
 * 跨窗口同步：别的窗口改了风格 → 后端 emit `config-changed` → config.load() 刷新
 * → styleTheme 变 → 此处 watch 触发重新打 data-theme，无需额外接线。
 *
 * 初次 apply 用 store 当前值（load() 前为默认 sci-fi，与现有深色观感一致，不会闪）。
 */
export function useTheme(): void {
  const store = useConfigStore()
  const apply = (id: string) => {
    document.documentElement.dataset.theme = getStyleTheme(id).id
  }
  apply(store.config.styleTheme)
  watch(() => store.config.styleTheme, apply)
}
