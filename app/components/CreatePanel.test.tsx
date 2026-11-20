import React, { Activity, act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Song, YueRequest } from '../types';

const composeScore = vi.fn();
const transcribe = vi.fn();
const instrumentalScore = vi.fn();
const recogniseLyrics = vi.fn();
let ready = true;
let configured = false;
vi.mock('../services/karaoke', async original => ({
  ...(await original<typeof import('../services/karaoke')>()),
  recogniseLyrics: (...args: unknown[]) => recogniseLyrics(...args),
}));

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({
  SELF_READ: {},
  libraryChanged: vi.fn(),
  useActivity: () => ({ data: [] }),
  useAssistantStatus: () => ({ data: null }),
  useSetupStatus: () => ({ isError: false, data: { ready, engine_ready: ready } }),
  useOpenRouterSettings: () => ({ data: { configured } }),
}));
vi.mock('../services/mcpBridge', () => ({ useBridgeCommand: () => undefined }));
vi.mock('../services/transcription', () => ({ composeScore: (...args: unknown[]) => composeScore(...args), composePlans: vi.fn(), transcribe: (...args: unknown[]) => transcribe(...args) }));
vi.mock('../services/scoreApi', () => ({
  instrumentalScore: (abc: string) => instrumentalScore(abc),
  failed: (answer: { error?: string }) => typeof answer?.error === 'string',
  chordBed: vi.fn(),
  matchSections: vi.fn(),
}));
vi.mock('../services/adapters', async (original) => ({
  ...(await original<typeof import('../services/adapters')>()),
  fetchAdapters: async () => ({ installed: [{ id: 'instrumental', origin: { type: 'catalog', catalog_id: 'yue2-instrumental' }, slots: ['ar', 'nar'], scales: {}, error: null }], installing: [], slots: [] }),
  installCatalogAdapters: vi.fn(),
  useAdapterLibrary: () => ({ installed: [], slots: [] }),
}));
vi.mock('./AdapterPicker', () => ({ AdapterPicker: () => null }));
vi.mock('./AudioWaveform', () => ({ AudioWaveform: () => null }));
vi.mock('./ScoreView', () => ({ ScoreView: () => null }));
vi.mock('./midi/MidiEditor', () => ({ MidiEditor: () => null }));
vi.mock('./MidiImportDialog', () => ({ MidiImportDialog: () => null }));
vi.mock('./PlaylistModals', () => ({ CreatePlaylistModal: () => null }));
vi.mock('./SlideToEnable', () => ({ SlideToEnable: () => null }));

const { CreatePanel } = await import('./CreatePanel');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
const sent: YueRequest[] = [];

beforeEach(() => {
  sent.length = 0;
  ready = true;
  configured = false;
  localStorage.clear();
  composeScore.mockReset();
  transcribe.mockReset().mockResolvedValue('X:1\nK:C\nC');
  instrumentalScore.mockReset();
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({}), { status: 200 })));
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });

function panel(props: Pick<React.ComponentProps<typeof CreatePanel>, 'musicEngine' | 'onMusicEngineChange'> & { initialData?: { song: Song; timestamp: number } | null; visible?: boolean; request?: React.ComponentProps<typeof CreatePanel>['request'] }) {
  return (
    <Activity mode={props.visible === false ? 'hidden' : 'visible'}>
      <CreatePanel musicEngine={props.musicEngine} onMusicEngineChange={props.onMusicEngineChange} onGenerate={request => sent.push(request)} isGenerating={false} request={props.request} initialData={props.initialData ?? null}
        playlists={[]} onCreatePlaylist={async () => null} playlistId="" onChoosePlaylist={() => undefined} />
    </Activity>
  );
}

const field = (name: string) => host.querySelector<HTMLTextAreaElement | HTMLInputElement>(name);
const type = (element: HTMLTextAreaElement | HTMLInputElement, value: string) => act(() => {
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), 'value')!.set!;
  setter.call(element, value);
  element.dispatchEvent(new Event('input', { bubbles: true }));
});
const click = (element: Element) => act(() => { element.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
const buttonNamed = (text: string) => [...host.querySelectorAll('button')].find(button => button.textContent?.includes(text));

describe('an instrumental (#217)', () => {
  it('keeps a score written for one submission out of the form and writes a new one for the next', async () => {
    act(() => root.render(panel({})));
    await flush();
    type(field('textarea[aria-label="styleLabel"], textarea')!, 'calm folk, acoustic guitar');
    const instrumental = host.querySelector<HTMLInputElement>('input[type="checkbox"]');
    click(instrumental!);
    await flush();
    composeScore.mockResolvedValueOnce('X:1\nPLAN-A').mockResolvedValueOnce('X:1\nPLAN-B');
    instrumentalScore.mockImplementation(async (abc: string) => ({ abc: `${abc}\nINSTR`, moved: 0, trimmed: 0, dropped: 0 }));
    click(buttonNamed('createInstrumental')!);
    await flush();
    click(buttonNamed('createInstrumental')!);
    await flush();
    expect(sent.map(request => request.abc)).toEqual(['X:1\nPLAN-A\nINSTR', 'X:1\nPLAN-B\nINSTR']);
    expect(composeScore).toHaveBeenCalledTimes(2);
  });

  it('honours No score: no score is written and the request goes with cot off', async () => {
    act(() => root.render(panel({})));
    await flush();
    type(field('textarea')!, 'calm folk, acoustic guitar');
    click(host.querySelector<HTMLInputElement>('input[type="checkbox"]')!);
    await flush();
    click(buttonNamed('cotOff')!);
    click(buttonNamed('createInstrumental')!);
    await flush();
    expect(composeScore).not.toHaveBeenCalled();
    expect(sent).toHaveLength(1);
    expect(sent[0].cot).toBe('off');
    expect(sent[0].abc).toBeUndefined();
    expect(sent[0].semantic_tokens).toBeUndefined();
  });
});

describe('selecting a second cover source', () => {
  it('keeps the edited style while a newer description is pending', async () => {
    const song = { id: 'cover', title: 'Cover', lyrics: '' } as Song;
    act(() => root.render(panel({ request: { id: 1, kind: 'transcribe', song, melodyOnly: true } })));
    await flush();
    const replies: ((response: Response) => void)[] = [];
    const signals: AbortSignal[] = [];
    vi.mocked(fetch).mockImplementation(async (url, options) => {
      if (!String(url).endsWith('/describe-style')) return Response.json({});
      signals.push(options!.signal!);
      return new Promise(resolve => replies.push(resolve));
    });
    const button = buttonNamed('describeByEar')!;
    const style = field('textarea[aria-label="styleCardTitle"]')!;
    click(button);
    type(style, 'my edited style');
    expect(signals[0].aborted).toBe(true);
    click(button);
    await act(async () => replies[0](Response.json({ style: 'stale result' })));
    expect(style.value).toBe('my edited style');
    expect(button.disabled).toBe(true);
    await act(async () => replies[1](Response.json({ style: 'new result' })));
    expect(style.value).toBe('new result');
    expect(button.disabled).toBe(false);
  });

  it('keeps the second score when the first recording finishes last', async () => {
    const first = { id: 'a', title: 'A', lyrics: '' } as Song;
    const second = { id: 'b', title: 'B', lyrics: '' } as Song;
    let finishFirst!: (score: string) => void;
    transcribe.mockImplementationOnce(() => new Promise<string>(resolve => { finishFirst = resolve; }));
    act(() => root.render(panel({ request: { id: 1, kind: 'transcribe', song: first, melodyOnly: true } })));
    await flush();
    const firstSignal = transcribe.mock.calls[0][2] as AbortSignal;
    transcribe.mockResolvedValueOnce('X:2\nK:D\nD');
    act(() => root.render(panel({ request: { id: 2, kind: 'transcribe', song: second, melodyOnly: true } })));
    await flush();
    expect(firstSignal.aborted).toBe(true);
    await act(async () => finishFirst('X:1\nK:C\nC'));
    expect([...host.querySelectorAll('textarea')].map(area => area.value)).toContain('X:2\nK:D\nD');
  });
});

describe('lyrics from a cover recording', () => {
  it('keeps text edited while recognition is running', async () => {
    const song = { id: 'cover', title: 'Cover', lyrics: '' } as Song;
    act(() => root.render(panel({ request: { id: 7, kind: 'transcribe', song, melodyOnly: true } })));
    await flush();
    let finish!: (words: string) => void;
    recogniseLyrics.mockImplementationOnce(() => new Promise<string>(resolve => { finish = resolve; }));
    click(buttonNamed('recogniseFromSource')!);
    expect(recogniseLyrics).toHaveBeenCalledWith({ songId: 'cover' });
    const lyrics = field('textarea[aria-label="lyrics"]')!;
    type(lyrics, 'my newer lyrics');
    await act(async () => finish('older recognition result'));
    expect(lyrics.value).toBe('my newer lyrics');
  });
});

describe('Reuse prompt (#216)', () => {
  const song = { id: 's', title: 'Song', style: 'pop', lyrics: '[verse]\nfirst words', generationParams: {} } as unknown as Song;

  it('keeps lyrics edited after a Reuse when Create is hidden and shown again; a new Reuse loads the song', async () => {
    act(() => root.render(panel({ initialData: { song, timestamp: 1 } })));
    await flush();
    const lyrics = () => [...host.querySelectorAll('textarea')].find(area => area.value.includes('words'));
    expect(lyrics()?.value).toContain('first words');
    type(lyrics()!, '[verse]\nedited words');
    act(() => root.render(panel({ initialData: { song, timestamp: 1 }, visible: false })));
    await flush();
    act(() => root.render(panel({ initialData: { song, timestamp: 1 } })));
    await flush();
    expect(lyrics()?.value).toContain('edited words');
    act(() => root.render(panel({ initialData: { song, timestamp: 2 } })));
    await flush();
    expect(lyrics()?.value).toContain('first words');
  });
});

describe('OpenRouter music creation', () => {
  const catalog = () => Response.json({ models: [
    { id: 'test/music', name: 'Music test model', capabilities: ['music_generation'] },
    { id: 'test/asr', name: 'Speech-only test model', capabilities: ['speech_to_text'] },
  ] });
  const select = (label: string) => [...host.querySelectorAll('select')]
    .find(control => control.getAttribute('aria-label') === label || control.closest('label')?.textContent?.startsWith(label))!;
  const choose = (control: HTMLSelectElement, value: string) => act(() => {
    control.value = value;
    control.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const create = () => [...host.querySelectorAll('button')].find(button => button.textContent?.trim() === 'create')!;

  it('submits the selected music model without local weights and preserves local settings when switching back', async () => {
    ready = false;
    configured = true;
    localStorage.setItem('studio.createSettings', JSON.stringify({ steps: '47' }));
    vi.mocked(fetch).mockImplementation(async url =>
      String(url) === '/v1/openrouter/catalog' ? catalog() : Response.json({}));
    const engineChange = vi.fn();
    act(() => root.render(panel({ musicEngine: 'local', onMusicEngineChange: engineChange })));
    await flush();
    expect(buttonNamed('Install local models')).toBeDefined();
    choose(select('Music engine'), 'openrouter');
    expect(engineChange).toHaveBeenCalledWith('openrouter');

    act(() => root.render(panel({ musicEngine: 'openrouter', onMusicEngineChange: engineChange })));
    await flush();
    expect(host.textContent).not.toContain('Speech-only test model');
    expect(create().disabled).toBe(true);
    choose(select('OpenRouter music model'), 'test/music');
    expect(localStorage.getItem('yue:openrouter-music-model')).toBe('test/music');
    type(field('textarea[aria-label="styleCardTitle"]')!, 'ambient soul, soft piano');
    click(create());
    expect(sent).toEqual([{ style: 'ambient soul, soft piano', lyrics: '', music_source: { kind: 'openrouter', model: 'test/music' } }]);

    ready = true;
    act(() => root.render(panel({ musicEngine: 'local', onMusicEngineChange: engineChange })));
    await flush();
    click(create());
    expect(sent[1]).toMatchObject({ music_source: { kind: 'local' }, style: 'ambient soul, soft piano', steps: 47 });
  });

  it('reports missing credentials, catalogue errors and an empty refreshed catalogue', async () => {
    const engineChange = vi.fn();
    act(() => root.render(panel({ musicEngine: 'openrouter', onMusicEngineChange: engineChange })));
    await flush();
    expect(buttonNamed('Open cloud settings')).toBeDefined();
    expect(create().disabled).toBe(true);
    expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url) === '/v1/openrouter/catalog')).toBe(false);

    configured = true;
    vi.mocked(fetch).mockImplementation(async url =>
      String(url) === '/v1/openrouter/catalog' ? Response.json({ error: 'offline' }, { status: 503 }) : Response.json({ models: [] }));
    act(() => root.render(panel({ musicEngine: 'openrouter', onMusicEngineChange: engineChange })));
    await flush();
    expect(host.textContent).toContain('Could not load OpenRouter music models.');
    click(buttonNamed('Refresh models')!);
    await flush();
    expect(vi.mocked(fetch)).toHaveBeenCalledWith('/v1/openrouter/catalog/refresh', expect.objectContaining({ method: 'POST' }));
    expect(host.textContent).toContain('No OpenRouter music models are available.');
    expect(create().disabled).toBe(true);
  });
});
