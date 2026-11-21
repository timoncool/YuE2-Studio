import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { RvcBurnTraining } from './RvcBurnTraining';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));

it('refreshes instead of waiting for progress when setup starts no download', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  let repaired = false;
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => {
    if (init?.method === 'POST') {
      repaired = true;
      return Response.json({ started: false });
    }
    return Response.json({
      ready: repaired, cuda: true, missing: repaired ? [] : ['ONNX Runtime CPU libraries'],
      installable: true, missing_bytes: repaired ? 0 : 80_000_000, download: null,
    });
  }));
  const host = document.createElement('div');
  const root = createRoot(host);
  const dataset = { id: 'dataset', name: 'Voice', trigger: '', created_at: '0', items: [] };
  const onError = vi.fn();
  try {
    await act(async () => root.render(<RvcBurnTraining dataset={dataset} busy={false} card="" control=""
      primary="" onStarted={vi.fn()} onError={onError} />));
    const install = [...host.querySelectorAll('button')].find(button => button.textContent?.includes('80'))!;
    await act(async () => install.click());
    expect(host.textContent).not.toContain('Installing');
    expect(host.querySelector('progress')).toBeNull();
    expect(install.isConnected).toBe(false);
    expect(onError).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});

it('downloads from the configured runtime source, shows progress after mounting, and cancels', async () => {
  vi.useFakeTimers();
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  let started = false;
  const request = vi.fn(async (_url: string, init?: RequestInit) => {
    if (init?.method === 'POST') {
      started = true;
      return Response.json({ started: true }, { status: 202 });
    }
    if (init?.method === 'DELETE') started = false;
    return Response.json({
      ready: false, cuda: true, missing: ['RVC Burn'], installable: true, missing_bytes: 900_000_000,
      download: started ? { downloaded_bytes: 500_000_000, total_bytes: 900_000_000, done: false } : null,
    });
  });
  vi.stubGlobal('fetch', request);
  const host = document.createElement('div');
  const root = createRoot(host);
  const dataset = { id: 'dataset', name: 'Voice', trigger: '', created_at: '0', items: [] };
  const onError = vi.fn();
  const render = () => root.render(<RvcBurnTraining dataset={dataset} busy={false} card="" control=""
    primary="" onStarted={vi.fn()} onError={onError} />);
  try {
    await act(async () => render());
    const download = [...host.querySelectorAll('button')].find(button => button.textContent?.includes('900'))!;
    await act(async () => download.click());
    await act(async () => vi.advanceTimersByTimeAsync(700));
    expect(host.querySelector('progress')?.value).toBe(500_000_000);
    expect(request.mock.calls.some(([url, init]) => url === '/v1/training/rvc-burn/setup' && init?.method === 'POST')).toBe(true);
    await act(async () => root.render(null));
    await act(async () => render());
    expect(host.querySelector('progress')?.value).toBe(500_000_000);
    const stop = [...host.querySelectorAll('button')].find(button => button.textContent === 'cancel')!;
    await act(async () => stop.click());
    expect(request.mock.calls.some(([url, init]) => url === '/v1/training/rvc-burn/setup' && init?.method === 'DELETE')).toBe(true);
    expect(onError).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});
