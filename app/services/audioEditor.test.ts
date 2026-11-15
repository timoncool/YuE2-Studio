import { afterEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { apiUrl } from './apiBase';
import { openAudioEditor } from './audioEditor';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

afterEach(() => {
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});

function shell(): void {
  (window as unknown as { __TAURI__?: unknown }).__TAURI__ = { core: { invoke } };
}

describe('the audio editor', () => {
  const audio = 'http://127.0.0.1:8791/v1/library/media/take-1';
  const page = apiUrl(`/editor/index.html?audioUrl=${encodeURIComponent(audio)}`);

  it('opens in a window of the studio on the desktop', async () => {
    shell();
    const tab = vi.fn();
    vi.stubGlobal('open', tab);
    vi.mocked(invoke).mockResolvedValue(undefined);

    await openAudioEditor(audio);

    expect(invoke).toHaveBeenCalledWith('open_audio_editor_window', { url: page });
    expect(tab).not.toHaveBeenCalled();
  });

  it('still edits in a browser tab outside the shell', async () => {
    const tab = vi.fn();
    vi.stubGlobal('open', tab);

    await openAudioEditor(audio);

    expect(tab).toHaveBeenCalledWith(page, '_blank', 'noopener');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('falls back to the browser when the shell has no window for it', async () => {
    shell();
    const tab = vi.fn();
    vi.stubGlobal('open', tab);
    vi.mocked(invoke).mockRejectedValue(new Error('unknown command'));

    await openAudioEditor(audio);

    expect(tab).toHaveBeenCalledWith(page, '_blank', 'noopener');
  });
});
