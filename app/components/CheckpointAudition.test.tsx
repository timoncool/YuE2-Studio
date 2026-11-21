import React, { act, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClientProvider } from '@tanstack/react-query';
import { expect, it, vi } from 'vitest';
import { queryClient, useLibraryPlaylists } from '../services/studioQueries';
import { PlaylistDetail } from './PlaylistDetail';
import { TrainingPanel } from './TrainingPanel';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ t: (key: string) => key, language: 'en', songCount: String }),
}));
vi.mock('./AudioWaveform', () => ({ AudioWaveform: () => null }));

it('opens a newly created training playlist and plays the media for the selected checkpoint step', async () => {
  const playlistId = 'checkpoint-listen-v1-run-id';
  const takes = ['Base', 'Step 1', 'Step 2'].map((title, step) => ({
    id: `take-${step}`, title, lyrics: 'Same lyrics', caption: 'Same style',
    audio_path: `take-${step}.flac`, updated_at: '1', created_at: '1',
  }));
  const run = {
    id: 'run-id', engine: 'yue2-cpp', dataset_id: 'set', dataset_name: 'Training songs',
    name: 'Checkpoint audition', trigger: '', status: 'done', stages: [],
    recipe: { steps: 2 }, steps: [], checkpoints: [1, 2], installed: [],
  };
  const state = {
    pack: [], pack_ready: true, card_trains: true, listen: { ready: true }, runs: [run],
    recipe_defaults: {}, recipe_fields: [], datasets: [{ id: 'set', name: 'Training songs', items: [] }],
  };
  const playlist = { id: playlistId, name: run.name, song_ids: takes.map(take => take.id), created_at: '1' };
  let finishPlaylists: ((response: Response) => void) | undefined;
  const refresh = vi.fn(() => new Promise<Response>(resolve => { finishPlaylists = resolve; }));
  const compare = vi.fn();
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
    if (url === '/v1/training') return Response.json(state);
    if (url === '/v1/training/runs/run-id/checkpoints/listen') {
      expect(init?.method).toBe('POST');
      compare();
      return Response.json({ playlist_id: playlistId });
    }
    if (url === '/v1/library/playlists') return refresh();
    if (url === '/v1/library/songs') return Response.json(takes);
    return Response.json({});
  });
  const audioStarted = vi.fn();
  const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(function (this: HTMLMediaElement) {
    if (!this.muted) audioStarted(this.getAttribute('src'));
    return Promise.resolve();
  });
  function Window() {
    useLibraryPlaylists();
    const [id, setId] = useState('');
    useEffect(() => {
      const openPlaylist = () => setId(window.location.pathname.slice('/playlist/'.length));
      window.addEventListener('popstate', openPlaylist);
      return () => window.removeEventListener('popstate', openPlaylist);
    }, []);
    return id ? <PlaylistDetail playlistId={id} currentSong={null} isPlaying={false}
      onBack={vi.fn()} onPlaySong={vi.fn()} onCreateHere={vi.fn()} onReuseHere={vi.fn()}
      onSelect={vi.fn()} onOpenLibraryTake={vi.fn()} notify={vi.fn()} onError={vi.fn()} /> : <TrainingPanel />;
  }
  const host = document.createElement('div');
  const root = createRoot(host);
  const path = window.location.pathname;
  const press = async (text: string) => {
    const button = [...host.querySelectorAll('button')].find(item => item.textContent?.includes(text));
    expect(button?.disabled).toBe(false);
    await act(async () => button!.click());
  };
  const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
  queryClient.setQueryData(['library', 'playlists'], []);
  try {
    await act(async () => root.render(<QueryClientProvider client={queryClient}><Window /></QueryClientProvider>));
    await press('Training songs');
    await press('3 · trainingStepResult');
    await press('Compare all checkpoints');
    expect(compare).toHaveBeenCalledOnce();
    expect(refresh).toHaveBeenCalledOnce();
    expect(window.location.pathname).toBe(path);
    await act(async () => finishPlaylists!(Response.json([playlist])));
    await flush();
    expect(window.location.pathname).toBe(`/playlist/${playlistId}`);
    const step = host.querySelector<HTMLSelectElement>('select[aria-label="B"]')!;
    expect(step.value).toBe('take-1');
    await act(async () => {
      step.value = 'take-2';
      step.dispatchEvent(new Event('change', { bubbles: true }));
    });
    const controls = host.querySelector('[role="tablist"]')?.parentElement;
    const playButton = controls?.querySelector<HTMLButtonElement>('button[aria-label="play"]');
    expect(playButton).toBeDefined();
    await act(async () => playButton!.click());
    expect(audioStarted).toHaveBeenCalledWith('/v1/library/media/take-2?v=1');
    expect(audioStarted).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
    queryClient.clear();
    window.history.replaceState({}, '', path);
    play.mockRestore();
    vi.unstubAllGlobals();
  }
});
