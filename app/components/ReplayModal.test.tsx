import { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { I18nProvider } from '../context/I18nContext';
import { mapNativeLibrarySong } from '../services/nativeLibrary';
import type { NativeLibrarySong } from '../types';
import { ReplayModal } from './ReplayModal';
import { SongDropdownMenu } from './SongDropdownMenu';
import { SongActionsProvider } from '../context/SongActionsContext';
import type { Song } from '../types';

vi.mock('../context/AuthContext', () => ({ useAuth: () => ({ user: null }) }));
vi.mock('../services/studioQueries', () => ({ useKaraokeStatus: () => ({ data: undefined }) }));
vi.mock('../services/rvcQuick', () => ({ useLastRvcVoice: () => null }));

const migrated = {
  id: 'migrated-track', title: 'Imported track', audio_path: 'track.flac',
  caption: '', lyrics: '', created_at: '1', replay_available: true,
};

function open(record: NativeLibrarySong) {
  return renderToStaticMarkup(<I18nProvider><ReplayModal
    song={mapNativeLibrarySong(record)} clientRef="qa" onClose={() => {}} onQueued={() => {}}
  /></I18nProvider>);
}

describe('saved model requirements in replay', () => {
  it('shows the archived ACE weights and the saved MiniMax profile', () => {
    const ace = open({
      ...migrated, engine_id: 'acestep-cpp',
      replay_model_files: ['ace-original.gguf', 'ace-lm.gguf', 'vae.safetensors'],
    });
    expect(ace).toContain('ace-original.gguf');
    expect(ace).toContain('ace-lm.gguf');
    expect(ace).toContain('vae.safetensors');
    expect(ace).not.toContain('+15 s');

    const minimax = open({
      ...migrated, engine_id: 'minimaxmusic-cpp', profile_id: 'quality-q8',
      replay_model_files: ['minimax-dit-q8.gguf'],
    });
    expect(minimax).toContain('quality-q8');
    expect(minimax).toContain('minimax-dit-q8.gguf');
    expect(minimax).not.toContain('+15 s');
  });

  it('offers a single HeartMuLa take with the saved step count and seed', () => {
    const heartmula = open({
      ...migrated, engine_id: 'audio.cpp', profile_id: 'heartmula_q8_0',
      generation_settings: { family: 'heartmula', steps: 4, seed: 42 },
    });
    expect(heartmula).toContain('HeartMuLa · audio.cpp');
    expect(heartmula).toContain('value="4"');
    expect(heartmula).toContain('placeholder="42"');
    expect(heartmula).not.toContain('+15 s');
    expect(heartmula).not.toContain('max="9"');
  });

  it('offers one packed YuE2 take with its original steps and no extension', () => {
    const yue = open({
      ...migrated, engine_id: 'audio.cpp', profile_id: 'yue2-3b-q8_0-nar-v9',
      generation_settings: { family: 'yue2', steps: 2, seed: 1221 },
    });
    expect(yue).toContain('YuE2 · audio.cpp');
    expect(yue).toContain('value="2"');
    expect(yue).toContain('max="100"');
    expect(yue).toContain('placeholder="1221"');
    expect(yue).not.toContain('+15 s');
    expect(yue).not.toContain('max="9"');
  });

  it('opens the replay dialog from the library song menu and submits its one saved YuE2 take', async () => {
    const song = mapNativeLibrarySong({
      ...migrated, engine_id: 'audio.cpp', profile_id: 'yue2-3b-q8_0-nar-v9',
      generation_settings: { family: 'yue2', steps: 2, seed: 1221, output_format: 'wav32' },
    });
    const queued = vi.fn();
    const fetch = vi.fn(async () => Response.json({ id: 'replayed' }));
    vi.stubGlobal('fetch', fetch);
    const mount = document.createElement('div');
    document.body.append(mount);
    const root = createRoot(mount);

    function LibraryReplay() {
      const [selected, select] = useState<Song>();
      return <SongActionsProvider value={{ replay: select }}>
        <SongDropdownMenu song={song} isOpen={!selected} onClose={() => {}} />
        {selected && <ReplayModal song={selected} clientRef="qa" onClose={() => select(undefined)} onQueued={queued} />}
      </SongActionsProvider>;
    }

    try {
      await act(async () => root.render(<I18nProvider><LibraryReplay /></I18nProvider>));
      const menu = [...mount.querySelectorAll('button')].find(button => /re-render/i.test(button.textContent ?? ''));
      expect(menu).toBeDefined();
      await act(async () => menu!.click());
      expect(mount.textContent).toContain('YuE2 · audio.cpp');
      expect(mount.querySelector<HTMLInputElement>('input[type="number"]')?.value).toBe('2');
      const submit = [...mount.querySelectorAll('button')].find(button => /re-render/i.test(button.textContent ?? ''));
      expect(submit).toBeDefined();
      await act(async () => submit!.click());
      expect(fetch).toHaveBeenCalledWith('/v1/music/replay', expect.objectContaining({
        body: JSON.stringify({ song_id: song.id, client_ref: 'qa', steps: 2, output_format: 'wav32' }),
      }));
      expect(queued).toHaveBeenCalledWith('replayed');
    } finally {
      await act(async () => root.unmount());
      mount.remove();
      vi.unstubAllGlobals();
    }
  });

  it('keeps the saved YuE2 profile visible when offering an extension', () => {
    const yue = open({ ...migrated, engine_id: 'yue2-cpp', profile_id: 'quality-q8' });
    expect(yue).toContain('quality-q8');
    expect(yue).toContain('+15 s');
  });
});
