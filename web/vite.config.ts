import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'

const server = process.env.FERRISTRACE_SERVER ?? 'http://localhost:4318'
const api = { target: server, changeOrigin: false }

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
  server: {
    proxy: {
      '^/auth/': api,
      '^/api/': api,
      '^/health$': api,
      '^/v1/': api,
    },
  },
})
