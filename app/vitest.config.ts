import { defineConfig } from 'vitest/config';
export default defineConfig({
  define: { __APP_VERSION__: JSON.stringify('test') },
  test: { environment: 'happy-dom', globals: true, include: ['**/*.test.ts', '**/*.test.tsx'], passWithNoTests: true },
});
