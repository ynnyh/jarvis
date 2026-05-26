import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

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
        writeHours: resolve(__dirname, 'desktop/writeHours.html'),
      },
    },
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, './desktop/src'),
    },
  },
  server: {
    port: 5174,
    strictPort: true,
  },
  clearScreen: false,
})
