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

function item(isCurrent: boolean, isPlaying: boolean, onPlay: () => void) {
  const noop = () => undefined;
  return (
    <QueryClientProvider client={queries}>
      <SongItem song={song} isCurrent={isCurrent} isSelected={false} isSelectionMode={false} isChecked={false} isLiked={false}
        isPlaying={isPlaying} isOwner={false} onPlay={onPlay} onSelect={noop} onToggleSelect={noop} onToggleLike={noop} onAddToPlaylist={noop} />
    </QueryClientProvider>
  );
}

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


describe('Suno-style multi-selection never steals the song details click', () => {
  it('opens details from a row but only selects from its checkbox', () => {
    const onSelect = vi.fn();
    const onToggleSelect = vi.fn();
    const noop = () => undefined;
    act(() => root.render(
      <QueryClientProvider client={queries}>
        <SongItem song={song} isCurrent={false} isSelected={false} isSelectionMode={true}
          isChecked={false} isLiked={false} isPlaying={false} isOwner={false}
          onPlay={noop} onSelect={onSelect} onToggleSelect={onToggleSelect}
          onToggleLike={noop} onAddToPlaylist={noop} />
      </QueryClientProvider>
    ));
    const row = host.querySelector<HTMLElement>('[data-mcp-context^="song "]');
    expect(row).not.toBeNull();
    act(() => row!.click());
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onToggleSelect).not.toHaveBeenCalled();

    const checkbox = host.querySelector<HTMLButtonElement>('button[aria-label="Select Neon nights"]');
    expect(checkbox).not.toBeNull();
    act(() => checkbox!.click());
    expect(onToggleSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});
