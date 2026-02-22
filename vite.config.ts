import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react-swc'
import { fileURLToPath, URL } from 'node:url'
import viteCompression from 'vite-plugin-compression'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const isSecureBuild = mode === 'secure'

  return {
    plugins: [
      react(),
      ...(isSecureBuild
        ? [
            viteCompression({ algorithm: 'gzip', ext: '.gz' }),
            viteCompression({ algorithm: 'brotliCompress', ext: '.br' }),
          ]
        : []),
    ],
    resolve: {
      alias: {
        'framer-motion': fileURLToPath(new URL('./src/shims/framer-motion.tsx', import.meta.url)),
      },
    },
    build: {
      sourcemap: !isSecureBuild,
      minify: 'terser',
      target: 'es2020',
      terserOptions: isSecureBuild
        ? {
            compress: {
              drop_console: true,
              drop_debugger: true,
              pure_funcs: ['console.info', 'console.debug', 'console.trace'],
              passes: 3,
            },
            mangle: {
              toplevel: true,
            },
            format: {
              comments: false,
            },
          }
        : {
            format: {
              comments: false,
            },
          },
    },
  }
})
