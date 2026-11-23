import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { expect, it, vi } from 'vitest';
import { EnginesAndUpdates } from './EnginesAndUpdates';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ t: (key: string) => key, language: 'en' }),
}));
vi.mock('./UpdateScreen', () => ({ UpdatePlanView: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('names the component catalogue, refreshes it and offers the native engines from it', async () => {
  let refreshed = 0;
  const request = vi.fn(async (url: string, options?: RequestInit) => {
    if (url === '/v1/components') return Response.json({
      sequence: 12, published: '2026-11-23', source: refreshed ? 'downloaded' : 'built-in', checked_at: refreshed ? 1_795_400_000 : null,
      refresh_error: refreshed ? null : 'no answer',
    });
    if (url === '/v1/components/refresh') {
      expect(options?.method).toBe('POST');
      refreshed += 1;
      return Response.json({ sequence: 12 });
    }
    if (url === '/v1/music/native/acestep-cpp/profiles') return Response.json({ engine_installed: false, profiles: [] });
    if (url === '/v1/music/native/acestep-cpp/runtime') return Response.json({ supported: true, installed: false, revision: 'd058cd86564abbad' });
    return Response.json({});
  });
  vi.stubGlobal('fetch', request);
  const host = document.createElement('div');
  const root = createRoot(host);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  try {
    await act(async () => root.render(<QueryClientProvider client={client}><EnginesAndUpdates onNavigate={vi.fn()} /></QueryClientProvider>));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    expect(host.textContent).toContain('Download list no. 12');
    expect(host.textContent).toContain('the one built into this version');
    expect(host.textContent).toContain('Could not check for a newer download list: the site did not answer.');
    expect(host.textContent).not.toContain('no answer.');
    expect(host.querySelector('[data-engine="acestep-cpp"]')?.textContent).toContain('d058cd86564a');
    const refresh = [...host.querySelectorAll('button')].find(button => button.textContent === 'refresh')!;
    await act(async () => refresh.click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    expect(refreshed).toBe(1);
    expect(host.textContent).not.toContain('Could not check for a newer download list');
    expect(request.mock.calls.some(([url]) => String(url).startsWith('/v1/components/music-engines'))).toBe(false);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
