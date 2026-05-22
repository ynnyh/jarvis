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
