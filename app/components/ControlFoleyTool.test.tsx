import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { ControlFoleyTool } from './ControlFoleyTool';
import { queryClient } from '../services/studioQueries';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  queryClient.removeQueries({ queryKey: ['controlfoley-model'] });
  vi.unstubAllGlobals();
});

it('uploads the original clip and sends the stored name to ControlFoley', async () => {
  const source = new File(['original video bytes'], 'scene.mp4', { type: 'video/mp4' });
  const fetcher = vi.fn(async (url: string, options?: RequestInit) => {
    if (url.includes('/v1/videos?name=')) {
      expect(options?.body).toBe(source);
      return Response.json({ name: 'stored-clip.mp4' });
    }
    return Response.json({ name: 'ControlFoley', ready: true, missing: [], missing_bytes: 0,
      license_url: 'https://huggingface.co/YJX-Xiaomi/ControlFoley',
      duration_seconds: { min: 0.2, max: 8, default: 8 },
    });
  });
  vi.stubGlobal('fetch', fetcher);
  const generate = vi.fn();
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<ControlFoleyTool card="" control="" busy={false} onGenerate={generate} />));
  const button = Array.from(host.querySelectorAll('button')).find(element => element.textContent === 'Generate sound')!;
  expect(button.disabled).toBe(true);
  const input = host.querySelector('input[type=file]')!;
  Object.defineProperty(input, 'files', { value: [source] });
  await act(async () => {
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await act(async () => button.click());
  expect(fetcher).toHaveBeenCalledWith(expect.stringContaining('/v1/videos?name=foley-'), { method: 'POST', body: source });
  expect(generate).toHaveBeenCalledWith({
    music_source: { kind: 'audio_cpp', model: 'controlfoley' }, controlfoley_video: 'stored-clip.mp4',
    style: '', lyrics: '', duration_seconds: 8, output_format: 'wav32',
  });
});
