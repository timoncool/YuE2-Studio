import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke, isTauri: () => true }));
vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));
import { WindowAudioCapture } from './WindowAudioCapture';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  vi.spyOn(window.navigator, 'userAgent', 'get').mockReturnValue('Test Windows');
  invoke.mockReset();
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const press = (label: string) => act(async () => {
  [...host.querySelectorAll('button')].find(button => button.textContent?.trim() === label)!.click();
});

it('records only the selected window and hands the WAV to the existing score input after Stop', async () => {
  let finish!: (recording: ArrayBuffer) => void;
  invoke.mockImplementation((command: string) => {
    if (command === 'audio_capture_windows') return Promise.resolve([{ id: '42', title: 'Score example' }, { id: '92', title: 'Other audio' }]);
    if (command === 'record_window_audio') return new Promise<ArrayBuffer>(resolve => { finish = resolve; });
    return Promise.resolve();
  });
  const onRecording = vi.fn();
  act(() => root.render(<WindowAudioCapture disabled={false} onRecording={onRecording} />));
  await press('Record window audio');
  expect(invoke).toHaveBeenCalledWith('audio_capture_windows');
  const select = host.querySelector('select')!;
  act(() => {
    select.value = '42';
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await press('Start recording');
  expect(invoke).toHaveBeenCalledWith('record_window_audio', { windowId: '42' });
  expect(onRecording).not.toHaveBeenCalled();
  await press('Stop and read score');
  expect(invoke).toHaveBeenCalledWith('stop_window_audio');
  await act(async () => finish(new Uint8Array([82, 73, 70, 70]).buffer));
  expect(onRecording).toHaveBeenCalledTimes(1);
  expect(onRecording.mock.calls[0][0]).toMatchObject({ name: 'window-audio.wav', type: 'audio/wav', size: 4 });
});
