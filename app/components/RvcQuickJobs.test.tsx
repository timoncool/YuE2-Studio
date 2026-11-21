import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { RvcQuickJobs } from './RvcQuickJobs';
import { RVC_QUICK_EVENT } from '../services/rvcQuick';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({ libraryChanged: vi.fn() }));

it('starts with the voice chosen in the library menu and cancels the same queued take', async () => {
  const job = { id: 'job-one', song_id: 'original', status: 'queued', take: null, error: null };
  const request = vi.fn(async (_url: string, init?: RequestInit) => {
    if (init?.method === 'POST') return Response.json(job);
    if (init?.method === 'DELETE') return Response.json({ ...job, status: 'cancelled' });
    return Response.json([]);
  });
  vi.stubGlobal('fetch', request);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const container = document.createElement('div');
  const root = createRoot(container);
  localStorage.setItem('studio.rvc.backend', 'audio.cpp');
  try {
    await act(async () => root.render(<RvcQuickJobs songs={[{ id: 'original', title: 'Original song' }]} />));
    await act(async () => { window.dispatchEvent(new CustomEvent(RVC_QUICK_EVENT, {
      detail: { songId: 'original', voiceId: 'remembered-voice' },
    })); });
    expect(request).toHaveBeenCalledWith('/v1/rvc/quick', expect.objectContaining({ method: 'POST',
      body: JSON.stringify({ song_id: 'original', voice_id: 'remembered-voice', backend: 'audio.cpp' }),
    }));
    expect(container.textContent).toContain('Original song · Queued');
    await act(async () => container.querySelector('button')?.click());
    expect(request).toHaveBeenCalledWith('/v1/rvc/quick/job-one', { method: 'DELETE' });
    expect(container.textContent).toContain('Original song · Cancelled');
  } finally {
    act(() => root.unmount());
    localStorage.removeItem('studio.rvc.backend');
    vi.unstubAllGlobals();
  }
});
