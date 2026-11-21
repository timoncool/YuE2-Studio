import { expect, it, vi } from 'vitest';
import { startBridge } from './mcpBridge';

it('reports the hint and current result associated with a visible comparison control', async () => {
  let receive!: (event: { data: string }) => Promise<void>;
  class Connection {
    set onmessage(handler: typeof receive) { receive = handler; }
  }
  const fetcher = vi.fn().mockResolvedValue(new Response('{}'));
  vi.stubGlobal('EventSource', Connection);
  vi.stubGlobal('fetch', fetcher);
  const area = document.createElement('section');
  document.body.append(area);
  area.innerHTML = '<button aria-describedby="hint score">Compare</button><span id="hint">Uses the chosen recogniser.</span><p id="score" role="status">A: 100% · B: 67%</p>';
  vi.spyOn(area.querySelector('button')!, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 100, 30));
  try {
    startBridge();
    await receive({ data: JSON.stringify({ window: 1 }) });
    await receive({ data: JSON.stringify({ window: 1, id: 'request', command: 'read_page' }) });
    const result = fetcher.mock.calls.find(([url]) => String(url).endsWith('/mcp/window/result'));
    expect(JSON.parse(result![1].body).result.text).toContain('Uses the chosen recogniser. A: 100% · B: 67%');
  } finally {
    area.remove();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  }
});
