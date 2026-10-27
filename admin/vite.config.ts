import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

// Served by the ferroshop binary under /admin; in dev the API is proxied.
export default defineConfig({
  base: '/admin/',
  plugins: [vue()],
  server: {
    proxy: {
      '/api': process.env.API_URL ?? 'http://localhost:8000',
    },
  },
})
