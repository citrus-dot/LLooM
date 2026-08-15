import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Dev server proxies API + SSE to the Rust lloom-server on :7861.
// Production builds to dist/, served statically by lloom-server.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': { target: 'http://localhost:7861', changeOrigin: true },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        manualChunks: {
          react: ['react', 'react-dom'],
          antd: ['antd', '@ant-design/icons'],
        },
      },
    },
  },
});
