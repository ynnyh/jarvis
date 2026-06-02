import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'
import { readFileSync } from 'node:fs'

// 在构建时读取 CHANGELOG.md 内容并内联到 bundle，避免 out-of-root ?raw import 的环境依赖问题
const CHANGELOG_MD = readFileSync(resolve(__dirname, 'CHANGELOG.md'), 'utf-8')

export default defineConfig({
  plugins: [vue()],
  root: './desktop',
  build: {
    outDir: '../dist/ui',
    emptyOutDir: true,
    minify: false,
    rollupOptions: {
      treeshake: false,
      input: {
        main: resolve(__dirname, 'desktop/index.html'),
        chat: resolve(__dirname, 'desktop/chat.html'),
        settings: resolve(__dirname, 'desktop/settings.html'),
        writeHours: resolve(__dirname, 'desktop/writeHours.html'),
        manualHours: resolve(__dirname, 'desktop/manualHours.html'),
        todayPlan: resolve(__dirname, 'desktop/todayPlan.html'),
        batchWrite: resolve(__dirname, 'desktop/batchWrite.html'),
        cost: resolve(__dirname, 'desktop/cost.html'),
      },
    },
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, './desktop/src'),
    },
  },
  define: {
    __CHANGELOG_MD__: JSON.stringify(CHANGELOG_MD),
  },
  server: {
    port: 5174,
    strictPort: true,
  },
  clearScreen: false,
})
