import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { useMusicPair, type PairSource, type PairStatus } from './musicPair';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('reopens the running transition and ignores a late answer for an older choice', async () => {
  const A = { kind: 'local' } as const;
  const B = { kind: 'audio_cpp', model: 'heartmula' } as const;
  const C = { kind: 'audio_cpp', model: 'mulacover' } as const;
  let current: PairStatus = { revision: 1, requested: A, active: null, stage: 'loading_model', elapsed_ms: 800 };
  const requests: { source: PairSource; sequence: number; client: string; reply: (response: Response) => void }[] = [];
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => {
    if (init?.method !== 'POST') return Response.json(current);
    const { music_source, sequence, client_id } = JSON.parse(String(init.body));
    return new Promise<Response>(reply => requests.push({ source: music_source, sequence, client: client_id, reply }));
  }));
  const root = createRoot(document.createElement('div'));
  let observation: ReturnType<typeof useMusicPair>;
  let selection: (next: PairSource) => void;
  function Panel() {
    const [source, setSource] = React.useState<PairSource>(A);
    const pair = useMusicPair(source, true);
    selection = next => { setSource(next); pair.select(); };
    observation = pair;
    return null;
  }
  try {
    await act(async () => root.render(<Panel />));
    expect(observation!.status).toEqual(current);
    expect(observation!.blocked).toBe(true);
    expect(requests).toHaveLength(0);
    await act(async () => selection!(B));
    await act(async () => selection!(C));
    expect(requests.map(request => request.source)).toEqual([B, C]);
    expect(requests[0].client).toBe(requests[1].client);
    expect(requests[1].sequence).toBeGreaterThan(requests[0].sequence);
    current = { revision: 3, requested: C, active: C, stage: 'ready', elapsed_ms: 30, load_on_create: true };
    await act(async () => requests[1].reply(Response.json(current)));
    await act(async () => requests[0].reply(Response.json({ revision: 2, requested: B, active: null,
      stage: 'needs_install', elapsed_ms: 10, missing_bytes: 500 })));
    expect(observation!.status).toEqual(current);
    expect(observation!.blocked).toBe(false);
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
