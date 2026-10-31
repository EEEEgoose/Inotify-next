import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// In development the Rust server runs on :8000; the built files are embedded into it.
const backend = 'http://127.0.0.1:8000'

export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: { '/api': backend, '/bark': backend },
  },
})
