import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    // En desarrollo, la API la sirve `cargo run -p buzon -- --config deploy/config.dev.toml`.
    proxy: { '/api': 'http://127.0.0.1:8080' },
  },
})
