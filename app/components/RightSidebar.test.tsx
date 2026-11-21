import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { RightSidebar } from './RightSidebar';
import type { Song } from '../types';
import { mapNativeLibrarySong } from '../services/nativeLibrary';

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

function mount(lyrics: string, details: Partial<Song> = {}) {
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  const song: Song = { id: 'song', title: 'A vocal recording', lyrics, style: '', coverUrl: '', duration: '1:00', createdAt: new Date(), isPublic: false, tags: [], ...details };
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<RightSidebar song={song} />));
}

function lyricsSection(): HTMLElement {
  return Array.from(host.querySelectorAll('button[aria-expanded]')).find(node => node.textContent === 'LYRICS')!.parentElement!.parentElement!;
}

function copyButton(section: HTMLElement): HTMLButtonElement {
  return Array.from(section.querySelectorAll<HTMLButtonElement>('button')).find(node => node.textContent?.includes('Copy'))!;
}

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.clearAllMocks();
});

it('shows the requested and actual durations of an older MiDasheng recording', () => {
  const saved = mapNativeLibrarySong({
    id: 'old-midas', title: 'Old MiDasheng recording', caption: 'Rain and piano', lyrics: '',
    audio_path: 'old.wav', created_at: '1', engine_id: 'audio.cpp', profile_id: 'q8_0',
    metadata: { duration_seconds: 1 },
    generation_settings: { music_source: { kind: 'audio_cpp', model: 'midashenglm_gen' }, duration_seconds: 5 },
  });
  mount('', saved);
  const params = Array.from(host.querySelectorAll<HTMLButtonElement>('button[aria-expanded]'))
    .find(node => node.textContent === 'songParamsBlock');
  expect(params).toBeDefined();
  act(() => params!.click());
  const rows = params!.parentElement!.parentElement!.textContent;
  expect(rows).toContain('Requested duration limit5 sec');
  expect(rows).toContain('Audio duration1 sec');
});

describe('song details lyrics', () => {
  it.each(['', '  \n '])('shows unknown saved text without classifying the recording as instrumental: %j', lyrics => {
    mount(lyrics);
    const section = lyricsSection();
    expect(section.textContent).toContain('No lyrics saved');
    expect(section.textContent).not.toContain('Instrumental');
    expect(copyButton(section).disabled).toBe(true);
  });

  it('shows and copies supplied multiline lyrics unchanged', async () => {
    const lyrics = '[Verse]\n Words with spaces  \nSecond line';
    mount(lyrics);
    const section = lyricsSection();
    expect(section.textContent).toContain(lyrics);
    expect(section.textContent).not.toContain('No lyrics saved');
    expect(copyButton(section).disabled).toBe(false);
    await act(async () => copyButton(section).click());
    expect(writeText).toHaveBeenCalledWith(lyrics);
  });
});
