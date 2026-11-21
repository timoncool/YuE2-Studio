// @vitest-environment happy-dom
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { I18nProvider } from '../context/I18nContext';
import { saveFile } from '../services/saveFile';
import { FilesPanel } from './FilesPanel';
import { ExportDialog, openExport } from './ExportDialog';

vi.mock('../services/externalLinks', () => ({ isDesktop: () => true }));

let host: HTMLDivElement;
let root: Root;
let dialog: HTMLDivElement;

beforeEach(async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ score: null, own: null })));
  localStorage.clear();
  localStorage.setItem('language', 'en');
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<I18nProvider><FilesPanel /><ExportDialog /></I18nProvider>));
  await act(async () => { openExport({ id: 'song', title: 'Сведение' }); });
  dialog = document.querySelector<HTMLDivElement>('[role="dialog"]')!;
  expect(dialog.classList.contains('z-80')).toBe(true);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const name = 'Сведение.wav';
const place = { id: 'save-1', path: 'D:/Экспорт/Сведение.wav', name };
const song = { url: '/v1/exports/song/file' };
const take = { blob: new Blob(['audio']) };

it.each([song, take])('leaves the files panel and alert empty when the dialog cancels', async (source) => {
  const fetch = vi.fn().mockResolvedValue(Response.json({ cancelled: true }));
  vi.stubGlobal('fetch', fetch);
  await act(async () => {
    expect(await saveFile(name, source)).toBe('cancelled');
  });
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(host.textContent).toBe('');
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(dialog.isConnected).toBe(true);
});

it.each([
  ['dialog failure', song, [{ error: 'Save dialog unavailable' }], 'Save dialog unavailable'],
  ['write failure', take, [place, { error: 'Destination is read-only' }], 'Destination is read-only'],
] as const)('announces a %s above the export modal and retains the file entry', async (_, source, replies, detail) => {
  const fetch = vi.fn();
  for (const [index, reply] of replies.entries()) {
    fetch.mockResolvedValueOnce(Response.json(reply, { status: index === replies.length - 1 ? 500 : 200 }));
  }
  vi.stubGlobal('fetch', fetch);
  await act(async () => {
    expect(await saveFile(name, source)).toBe('failed');
  });
  const alert = document.querySelector('[role="alert"]');
  expect(alert?.textContent).toContain(`${name}: ${detail}`);
  expect(alert?.parentElement?.parentElement).toBe(document.body);
  expect(alert?.parentElement?.classList.contains('z-100')).toBe(true);
  expect(host.textContent).toContain(detail);
  expect(dialog.isConnected).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(replies.length);

  await act(async () => alert?.querySelector('button')?.click());
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(host.textContent).toContain(detail);
});

it('cancels the ZIP save from the actual export dialog without uploading the archive', async () => {
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json({ file_name: 'Сведение.zip', url: '/v1/exports/tracks/file' }))
    .mockResolvedValueOnce(Response.json({ cancelled: true }));
  vi.stubGlobal('fetch', fetch);
  const zip = Array.from(dialog.querySelectorAll('button')).find(button => button.textContent?.includes('ZIP'))!;
  await act(async () => zip.click());
  expect(fetch.mock.calls.map(([url]) => url)).toEqual(['/v1/exports/tracks', '/v1/files/save']);
  expect(fetch.mock.calls[1]?.[1]?.body).toBe(JSON.stringify({ name: 'Сведение.zip' }));
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(host.textContent).toBe('');
  expect(dialog.isConnected).toBe(true);
});
