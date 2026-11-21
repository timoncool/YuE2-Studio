import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { I18nProvider } from '../context/I18nContext';
import { mapNativeLibrarySong } from '../services/nativeLibrary';
import type { NativeLibrarySong } from '../types';
import { ReplayModal } from './ReplayModal';

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

  it('keeps the saved YuE2 profile visible when offering an extension', () => {
    const yue = open({ ...migrated, engine_id: 'yue2-cpp', profile_id: 'quality-q8' });
    expect(yue).toContain('quality-q8');
    expect(yue).toContain('+15 s');
  });
});
