import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';

let hubItems: { id: string; kind: string }[] = [];
vi.mock('./studioQueries', () => ({ useHubState: () => ({ data: { items: hubItems } }) }));
const { markNewsSeen, useNewsIds, useUnseenNews } = await import('./newsSeen');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let count = -1;
let ids: string[] = [];
function Probe() {
  count = useUnseenNews('en');
  ids = useNewsIds('en');
  return null;
}

afterEach(() => { localStorage.clear(); hubItems = []; });

it('counts nothing on a first launch, then each news item that arrives until the news page is opened', async () => {
  const host = document.createElement('div');
  const root = createRoot(host);
  await act(async () => root.render(<Probe />));
  expect(count, 'what the release ships is not news on its first launch').toBe(0);

  hubItems = [{ id: 'hub-1', kind: 'news' }, { id: 'notice-1', kind: 'notice' }];
  await act(async () => root.render(<Probe />));
  expect(count).toBe(1);

  await act(async () => markNewsSeen(ids));
  expect(count, 'opening the news page reads them').toBe(0);
  await act(async () => root.unmount());
});
