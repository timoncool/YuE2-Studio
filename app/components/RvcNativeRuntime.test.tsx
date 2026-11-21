import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { RvcNativeRuntime } from './RvcNativeRuntime';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));

it('can retry a failed runtime status check without starting another download', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const fetch = vi.fn().mockResolvedValueOnce(Response.json({ error: 'Service restarting' }, { status: 503 }))
    .mockResolvedValueOnce(Response.json({ supported: true, installed: false, missing_bytes: 42_000_000, download: null }));
  vi.stubGlobal('fetch', fetch);
  const host = document.createElement('div');
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RvcNativeRuntime onInstalled={vi.fn()} disabled={false} />));
    expect(host.querySelector('[role=alert]')?.textContent).toContain('Service restarting');
    expect(host.querySelector('button')?.textContent).toBe('Retry checking components');
    await act(async () => host.querySelector('button')!.click());
    expect(host.querySelector('[role=alert]')).toBeNull();
    expect(host.querySelector('button')?.textContent).toContain('Install vocal-fast-rs');
    expect(fetch.mock.calls.map(([, init]) => init?.method ?? 'GET')).toEqual(['GET', 'GET']);
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
