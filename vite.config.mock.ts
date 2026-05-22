import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

export default defineConfig({
  plugins: [vue()],
  root: './desktop',
  build: {
    outDir: '../dist-web',
    emptyOutDir: true,
    rollupOptions: {
      input: resolve(__dirname, 'desktop/index.mock.html'),
    },
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, './desktop/src'),
    },
  },
  server: {
    port: 5173,
    open: true,
  },
})
