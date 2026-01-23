import { defineConfig } from 'vite'

export default defineConfig({
  build: {
    // Wails ожидает файлы в папке dist
    outDir: 'dist',
    rollupOptions: {
      input: {
        // Точка входа - наш index.html
        main: 'index.html',
      },
    },
  },
})