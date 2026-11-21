import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { SoundEffectsTool } from './SoundEffectsTool';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ language: 'en' }),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.unstubAllGlobals();
});

it('keeps Create disabled until the effect model is ready, then sends the description', async () => {
  const source = 'https://huggingface.co/audio-cpp/audio.cpp-gguf/blob/example/moss-soundeffect-v2-q8_0.gguf';
  let ready = false;
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({
    id: 'moss_soundeffect', name: 'MOSS-SoundEffect v2', ready,
    missing: ready ? [] : ['moss-soundeffect-v2-q8_0.gguf'],
    missing_bytes: ready ? 0 : 4_001_175_520, model_directory: 'audiocpp/models/moss_soundeffect',
    package_url: source, license_url: 'https://huggingface.co/OpenMOSS-Team/MOSS-SoundEffect-v2.0',
    duration_seconds: { min: 0.1, max: 30, default: 10 },
  })));
  const generate = vi.fn();
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<SoundEffectsTool card="" control="" busy={false} onGenerate={generate} />));
  const button = (name: string) => Array.from(host.querySelectorAll('button')).find(el => el.textContent === name)!;
  expect(host.querySelector(`a[href="${source}"]`)).not.toBeNull();
  expect(button('Generate sound effect').disabled).toBe(true);

  ready = true;
  await act(async () => button('Refresh models').click());
  const description = host.querySelector('textarea')!;
  const set = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(description), 'value')!.set!;
  act(() => {
    set.call(description, 'Ceramic cup tapping on a wooden table');
    description.dispatchEvent(new Event('input', { bubbles: true }));
  });
  expect(button('Generate sound effect').disabled).toBe(false);
  act(() => button('Generate sound effect').click());
  expect(generate).toHaveBeenCalledWith({
    music_source: { kind: 'audio_cpp', model: 'moss_soundeffect' },
    style: 'Ceramic cup tapping on a wooden table', lyrics: '', duration_seconds: 10, output_format: 'wav32',
  });
});
