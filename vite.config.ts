import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react-swc'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig(({ mode }) => {
  const isSecureBuild = mode === 'secure'

  return {
    plugins: [react()],
    resolve: {
      alias: {
        'framer-motion': fileURLToPath(new URL('./src/shims/framer-motion.tsx', import.meta.url)),
      },
    },
    build: {
      sourcemap: !isSecureBuild,
      minify: 'esbuild',
      target: 'es2020',
      rollupOptions: {
        output: {
          manualChunks: {
            'vendor-react': ['react', 'react-dom'],
            'vendor-tauri': ['@tauri-apps/api/core', '@tauri-apps/api/event'],
          },
        },
      },
    },
  }
})
