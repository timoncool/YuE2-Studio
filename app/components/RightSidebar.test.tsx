import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { RightSidebar } from './RightSidebar';
import type { Song } from '../types';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => ({ lyricsSection: 'LYRICS', noLyricsSaved: 'No lyrics saved', instrumental: 'Instrumental', copy: 'Copy' }[key] ?? key) }) }));
vi.mock('../context/AuthContext', () => ({ useAuth: () => ({ user: null }) }));
vi.mock('../context/SongActionsContext', () => ({ useSongActions: () => ({}) }));
vi.mock('../services/adapters', () => ({ useAdapterLibrary: () => ({ installed: [], slots: [] }), usesFromSettings: () => [], localized: () => '' }));
vi.mock('../services/studioQueries', () => ({ useKaraokeStatus: () => ({ data: { ready: false } }) }));
vi.mock('./SongDropdownMenu', () => ({ SongDropdownMenu: () => null }));
vi.mock('./KaraokeAction', () => ({ KaraokeAction: () => null }));
vi.mock('./AlbumCover', () => ({ SongCover: () => <div>Cover</div> }));
vi.mock('./ScoreView', () => ({ ScoreView: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
const writeText = vi.fn(async () => undefined);

function mount(lyrics: string) {
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  const song: Song = { id: 'song', title: 'A vocal recording', lyrics, style: '', coverUrl: '', duration: '1:00', createdAt: new Date(), isPublic: false, tags: [] };
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<RightSidebar song={song} />));
}

function lyricsSection(): HTMLElement {
  return Array.from(host.querySelectorAll('h3')).find(node => node.textContent === 'LYRICS')!.parentElement!.parentElement!;
}

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.clearAllMocks();
});

describe('song details lyrics', () => {
  it.each(['', '  \n '])('shows unknown saved text without classifying the recording as instrumental: %j', lyrics => {
    mount(lyrics);
    const section = lyricsSection();
    expect(section.textContent).toContain('No lyrics saved');
    expect(section.textContent).not.toContain('Instrumental');
    expect(section.querySelector('button')!.disabled).toBe(true);
  });

  it('shows and copies supplied multiline lyrics unchanged', async () => {
    const lyrics = '[Verse]\n Words with spaces  \nSecond line';
    mount(lyrics);
    const section = lyricsSection();
    expect(section.textContent).toContain(lyrics);
    expect(section.textContent).not.toContain('No lyrics saved');
    expect(section.querySelector('button')!.disabled).toBe(false);
    await act(async () => section.querySelector<HTMLButtonElement>('button')!.click());
    expect(writeText).toHaveBeenCalledWith(lyrics);
  });
});
