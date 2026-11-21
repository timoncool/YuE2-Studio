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

it('accepts idle revision zero after a service restart without posting another selection', async () => {
  const local = { kind: 'local' } as const;
  const fromOld: PairStatus = { instance_id: 'old-service', revision: 5, requested: local, active: null,
    stage: 'loading_model', elapsed_ms: 1000 };
  const fromNew: PairStatus = { instance_id: 'new-service', revision: 0, requested: null, active: null,
    stage: 'idle', elapsed_ms: 0 };
  let restarted = false;
  let pending: ((response: Response) => void) | undefined;
  let read = 0;
  const fetcher = vi.fn(async (_url: string, init?: RequestInit) => {
    expect(init?.method).not.toBe('POST');
    if (restarted) return Response.json(fromNew);
    if (read++ === 0) return Response.json(fromOld);
    return new Promise<Response>(reply => { pending = reply; });
  });
  vi.stubGlobal('fetch', fetcher);
  const root = createRoot(document.createElement('div'));
  let observation: ReturnType<typeof useMusicPair>;
  function Panel({ instance }: { instance: string }) {
    observation = useMusicPair(local, true, instance);
    return null;
  }
  try {
    await act(async () => root.render(<Panel instance="old-service" />));
    expect(observation!.status).toEqual(fromOld);
    expect(observation!.blocked).toBe(true);
    expect(pending).toBeDefined();
    restarted = true;
    await act(async () => root.render(<Panel instance="new-service" />));
    expect(observation!.status).toEqual(fromNew);
    expect(observation!.blocked).toBe(false);
    await act(async () => pending?.(Response.json({ ...fromOld, revision: 6 })));
    expect(observation!.status).toEqual(fromNew);
    expect(fetcher.mock.calls.every(([, init]) => init?.method !== 'POST')).toBe(true);
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});

it('keeps ready when an older snapshot with the same revision arrives after polling', async () => {
  const local = { kind: 'local' } as const;
  const loading: PairStatus = { instance_id: 'same-service', revision: 5, requested: local, active: null,
    stage: 'loading_model', elapsed_ms: 1000 };
  let current = loading;
  let postReply: ((response: Response) => void) | undefined;
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => init?.method !== 'POST'
    ? Response.json(current) : new Promise<Response>(reply => { postReply = reply; })));
  const root = createRoot(document.createElement('div'));
  let observation: ReturnType<typeof useMusicPair>;
  function Panel() {
    observation = useMusicPair(local, true, 'same-service');
    return null;
  }
  try {
    await act(async () => root.render(<Panel />));
    await act(async () => observation!.select());
    expect(postReply).toBeDefined();
    current = { ...loading, active: local, stage: 'ready', elapsed_ms: 1200 };
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 450)); });
    expect(observation!.status).toEqual(current);
    await act(async () => postReply?.(Response.json({ ...loading, stage: 'checking_files', elapsed_ms: 0 })));
    expect(observation!.status).toEqual(current);
    expect(observation!.blocked).toBe(false);
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
