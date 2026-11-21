import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { Song } from '../types';

const queries = new QueryClient();

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../context/AuthContext', () => ({ useAuth: () => ({ user: null }) }));

const { SongItem } = await import('./SongList');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const song = { id: 's1', title: 'Neon nights', style: 'synth-pop', lyrics: '', tags: [], duration: '1:00', audioUrl: '/a.flac', createdAt: new Date(0) } as unknown as Song;

function item(isCurrent: boolean, isPlaying: boolean, onPlay: () => void, track = song) {
  const noop = () => undefined;
  return (
    <QueryClientProvider client={queries}>
      <SongItem song={track} isCurrent={isCurrent} isSelected={false} isSelectionMode={false} isChecked={false} isLiked={false}
        isPlaying={isPlaying} isOwner={false} onPlay={onPlay} onSelect={noop} onToggleSelect={noop} onToggleLike={noop} onAddToPlaylist={noop} />
    </QueryClientProvider>
  );
}

it.each([
  ['Studio Song Pro', 'Studio Song Pro'],
  [undefined, 'provider/song-model'],
])('shows the recorded cloud model name with a fallback for older takes (%s)', (name, shown) => {
  act(() => root.render(item(false, false, vi.fn(), {
    ...song, ditModel: 'openrouter', generationParams: { model: 'provider/song-model', model_name: name },
  })));
  expect(host.textContent).toContain('OpenRouter');
  expect(host.textContent).toContain(shown);
  expect(host.querySelector('[title="provider/song-model"]')).not.toBeNull();
  expect(host.textContent).not.toContain('YuE2');
});

it.each([
  ['acestep-cpp', 'ACE-Step'],
  ['minimaxmusic-cpp', 'MiniMax Music 3'],
])('displays the original engine on a migrated %s take', (ditModel, label) => {
  act(() => root.render(item(false, false, vi.fn(), { ...song, ditModel, lmModel: 'balanced' })));
  expect(host.textContent).toContain(label);
  expect(host.textContent).not.toContain('YuE2');
});

it('shows audio.cpp on a Music3 card while the song is being generated', () => {
  act(() => root.render(item(false, false, vi.fn(), {
    ...song, ditModel: 'audio.cpp', audioUrl: undefined, isGenerating: true, stage: 'stageGeneratingTrack',
  })));
  expect(host.textContent).toContain('audio.cpp');
  expect(host.textContent).not.toContain('YuE2');
});

describe('the play control of a song row (YuE2 Studio #171)', () => {
  it('is a button named by the song, pressed while it plays, that starts the song from the keyboard', () => {
    const onPlay = vi.fn();
    act(() => root.render(item(false, false, onPlay)));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="play: Neon nights"]');
    expect(button).not.toBeNull();
    expect(button!.type).toBe('button');
    expect(button!.getAttribute('aria-pressed')).toBe('false');
    act(() => { button!.click(); });
    expect(onPlay).toHaveBeenCalledTimes(1);

    act(() => root.render(item(true, true, onPlay)));
    const playing = host.querySelector<HTMLButtonElement>('button[aria-label="pause: Neon nights"]');
    expect(playing?.getAttribute('aria-pressed')).toBe('true');
  });
});
