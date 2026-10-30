/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Built ahead of time into dist/, which the resonance binary embeds (MVP-043). Vite is a build
// tool here, never a server for the real UI. `npm run dev` proxies the API to a running daemon
// (RESONANCE_UI, for example http://127.0.0.1:56094) for working on the UI itself.
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    assetsDir: 'assets',
    sourcemap: false,
    chunkSizeWarningLimit: 2000,
  },
  server: {
    proxy: process.env.RESONANCE_UI
      ? { '/api': { target: process.env.RESONANCE_UI, changeOrigin: true }, '/auth': process.env.RESONANCE_UI }
      : undefined,
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    css: false,
  },
});
