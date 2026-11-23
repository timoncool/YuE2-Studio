import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { YueModelSetPicker } from './YueModelSetPicker';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  vi.stubGlobal('fetch', vi.fn(async (url: string) => Response.json(
    url.endsWith('/setup/catalog') ? {
      recommended_profile_id: 'light',
      profiles: [
        { id: 'light', label: 'Light', labels: { en: 'Light' }, components: ['backbone-q4', 'vae'], total_bytes: 2 },
        { id: 'max', label: 'Max', labels: { en: 'Max' }, components: ['backbone-q8', 'vae'], total_bytes: 3 },
      ],
      components: [{ id: 'backbone-q8', partial_bytes: 5 }],
    } : url.endsWith('/setup/status') ? { selected_profile_id: 'light', installed_components: ['backbone-q4', 'vae'], components: [] }
    : {},
  )));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

it('deletes a downloaded YuE2 set and a half-finished one from Settings', async () => {
  act(() => root.render(<YueModelSetPicker manage onChanged={vi.fn()} />));
  await flush();
  expect(host.textContent).toContain('partly downloaded');
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Delete the Light set"]')!.click());
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Delete the partial download"]')!.click());
  const removed = vi.mocked(fetch).mock.calls.filter(([url]) => String(url).endsWith('/setup/remove')).map(([, init]) => JSON.parse(String(init?.body)));
  expect(removed).toEqual([{ ids: ['backbone-q4', 'vae'] }, { ids: ['backbone-q8'] }]);
});

it('only chooses in the create form', async () => {
  act(() => root.render(<YueModelSetPicker onChanged={vi.fn()} />));
  await flush();
  expect(host.querySelector('button[aria-label^="Delete"]')).toBeNull();
});
