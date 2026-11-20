import React, { Activity, act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Song, YueRequest } from '../types';
import { useGenerations } from '../services/generations';

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
vi.mock('../services/studioQueries', async original => ({
  ...await original<typeof import('../services/studioQueries')>(),
  useLibrarySongs: () => ({ data: [] }),
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

function panel(props: Pick<React.ComponentProps<typeof CreatePanel>, 'musicEngine' | 'onMusicEngineChange' | 'activeJobCount'> & { initialData?: { song: Song; timestamp: number } | null; visible?: boolean; request?: React.ComponentProps<typeof CreatePanel>['request']; onGenerate?: (request: YueRequest) => void }) {
  return (
    <Activity mode={props.visible === false ? 'hidden' : 'visible'}>
      <CreatePanel musicEngine={props.musicEngine} onMusicEngineChange={props.onMusicEngineChange} onGenerate={props.onGenerate ?? (request => sent.push(request))} isGenerating={Boolean(props.activeJobCount)} activeJobCount={props.activeJobCount} request={props.request} initialData={props.initialData ?? null}
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

it('keeps an edited score across a cloud session and reopening the native form', async () => {
  localStorage.setItem('studio.createDraft', JSON.stringify({ style: 'folk', lyrics: 'verse' }));
  act(() => root.render(panel({ musicEngine: 'local' })));
  await flush();
  const score = 'X:1\nK:C\nC D E F|';
  type(field('textarea[aria-label="scoreCardTitle"]')!, score);
  click(buttonNamed('cotMelody')!);
  act(() => root.render(panel({ musicEngine: 'openrouter' })));
  await flush();
  act(() => root.unmount());
  root = createRoot(host);
  act(() => root.render(panel({ musicEngine: 'local' })));
  await flush();
  expect(field('textarea[aria-label="scoreCardTitle"]')!.value).toBe(score);
  expect(JSON.parse(localStorage.getItem('studio.createDraft')!)).toMatchObject({ style: 'folk', lyrics: 'verse', abc: score, cot: 'melody' });
  click(host.querySelector('button[title="resetPrompt"]')!);
  expect(JSON.parse(localStorage.getItem('studio.createDraft')!)).toMatchObject({ style: '', lyrics: '', abc: '', cot: '', continueScore: '' });
});

it('keeps settings by native engine and profile and submits the selected pair', async () => {
  const score = 'X:1\nK:C\nC D E F|';
  localStorage.setItem('studio.createDraft', JSON.stringify({ style: 'folk', lyrics: 'shared words', abc: score }));
  const profiles = [
    { id: 'balanced', label: 'Balanced', total_bytes: 10, ready: true, missing: [] },
    { id: 'quality', label: 'Quality', total_bytes: 15, ready: true, missing: [] },
    { id: 'missing', label: 'Missing weights', total_bytes: 20, ready: false, missing: ['model.gguf'] },
  ];
  let aceInstalled = false;
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    const engine = url.match(/^\/v1\/music\/native\/([^/]+)\/profiles$/)?.[1];
    return Response.json(engine ? {
      engine, engine_installed: engine === 'minimaxmusic-cpp' || aceInstalled, profiles,
    } : {});
  }));
  const setup = async (engine: NonNullable<React.ComponentProps<typeof CreatePanel>['musicEngine']>) => {
    act(() => root.render(panel({ musicEngine: engine })));
    await flush();
  };
  const select = (name: string) => act(() => {
    const model = host.querySelector<HTMLSelectElement>('select[aria-label="Local model"]')!;
    model.value = name;
    model.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const number = (label: string) =>
    [...host.querySelectorAll('label')].find(node => node.textContent?.trim() === label)!.querySelector('input')!;
  const create = () => buttonNamed('create') as HTMLButtonElement;
  await setup('minimaxmusic-cpp');
  select('missing');
  expect(create().disabled).toBe(true);
  expect(host.textContent).toContain('model.gguf');
  select('balanced');
  type(number('Duration (seconds)'), '36');
  type(number('Language model seed'), '4');
  expect(host.textContent).toContain('The draft keeps your ABC score.');
  click(create());
  expect(sent.at(-1)).toEqual({
    music_source: { kind: 'native', engine: 'minimaxmusic-cpp', model: 'balanced' },
    style: 'folk', lyrics: 'shared words', duration_seconds: 36, lm_seed: 4, output_format: 'flac',
  });
  select('quality');
  expect(number('Duration (seconds)').value).toBe('');
  type(number('Duration (seconds)'), '48');
  await setup('acestep-cpp');
  select('balanced');
  expect(host.textContent).toContain('This engine is not installed.');
  expect(create().disabled).toBe(true);
  aceInstalled = true;
  click(buttonNamed('Refresh models')!);
  await flush();
  expect(number('Duration (seconds)').value).toBe('');
  type(number('CFG'), '2.5');
  type(number('Steps'), '8');
  click(create());
  expect(sent.at(-1)).toEqual({
    music_source: { kind: 'native', engine: 'acestep-cpp', model: 'balanced' },
    style: 'folk', lyrics: 'shared words', steps: 8, ace: { guidance_scale: 2.5 },
  });

  act(() => root.unmount());
  root = createRoot(host);
  await setup('minimaxmusic-cpp');
  expect(number('Duration (seconds)').value).toBe('48');
  select('balanced');
  expect(number('Duration (seconds)').value).toBe('36');
  expect(number('Language model seed').value).toBe('4');
  await setup('local');
  expect(field('textarea[aria-label="scoreCardTitle"]')!.value).toBe(score);

  act(() => root.render(panel({ musicEngine: 'minimaxmusic-cpp', activeJobCount: 2 })));
  await flush();
  expect(create().textContent).toContain('2');
  click(host.querySelector('button[aria-label="resetCreateForm"]')!);
  expect(create().textContent).toContain('2');
  expect(number('Duration (seconds)').value).toBe('');
  expect(number('Language model seed').value).toBe('');
  expect(host.querySelector<HTMLSelectElement>('select[aria-label="Local model"]')!.value).toBe('balanced');
  expect(field('textarea[aria-label="lyrics"]')!.value).toBe('');
  expect(sent).toHaveLength(2);

  select('quality');
  expect(number('Duration (seconds)').value).toBe('48');
  await setup('acestep-cpp');
  expect(number('Steps').value).toBe('8');
  await setup('local');
  expect(field('textarea[aria-label="scoreCardTitle"]')!.value).toBe('');
});

it('posts the remembered MiniMax output format and keeps it out of ACE requests', async () => {
  vi.stubGlobal('EventSource', class { close() {} });
  const profiles = ['balanced', 'quality'].map(id => ({ id, label: id, ready: true, missing: [], total_bytes: 1 }));
  const posted: YueRequest[] = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    const engine = url.match(/^\/v1\/music\/native\/([^/]+)\/profiles$/)?.[1];
    if (engine) return Response.json({ engine, engine_installed: true, profiles });
    if (url === '/v1/music/jobs' && init?.method === 'POST') {
      posted.push(JSON.parse(String(init.body)));
      return Response.json({ id: `job-${posted.length}`, status: 'completed', songs: [] });
    }
    return Response.json([]);
  }));
  localStorage.setItem('studio.createDraft', JSON.stringify({ style: 'folk' }));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function Editor({ engine }: { engine: 'minimaxmusic-cpp' | 'acestep-cpp' }) {
    const { generate } = useGenerations({ enabled: false, notify: () => {}, onFinished: () => {} });
    return panel({ musicEngine: engine, onGenerate: generate });
  }
  const setup = async (engine: 'minimaxmusic-cpp' | 'acestep-cpp') => {
    act(() => root.render(<QueryClientProvider client={client}><Editor engine={engine} /></QueryClientProvider>));
    await flush();
  };
  const choose = (select: HTMLSelectElement, value: string) => act(() => {
    select.value = value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const model = () => host.querySelector<HTMLSelectElement>('select[aria-label="Local model"]')!;
  const format = () => Array.from(host.querySelectorAll('label')).find(label => label.textContent?.startsWith('outputFormat'))?.querySelector('select');
  const submit = async () => { click(buttonNamed('create')!); await flush(); };

  await setup('minimaxmusic-cpp');
  choose(model(), 'balanced');
  expect(format()?.value).toBe('flac');
  await submit();
  choose(format()!, 'wav32');
  await submit();
  choose(model(), 'quality');
  expect(format()?.value).toBe('flac');
  choose(format()!, 'mp3');
  await submit();
  await setup('acestep-cpp');
  choose(model(), 'balanced');
  expect(format()).toBeUndefined();
  await submit();
  await setup('minimaxmusic-cpp');
  expect(model().value).toBe('quality');
  expect(format()?.value).toBe('mp3');
  choose(model(), 'balanced');
  expect(format()?.value).toBe('wav32');
  await submit();

  expect(posted.map(({ music_source, output_format }) => [music_source, output_format])).toEqual([
    [{ kind: 'native', engine: 'minimaxmusic-cpp', model: 'balanced' }, 'flac'],
    [{ kind: 'native', engine: 'minimaxmusic-cpp', model: 'balanced' }, 'wav32'],
    [{ kind: 'native', engine: 'minimaxmusic-cpp', model: 'quality' }, 'mp3'],
    [{ kind: 'native', engine: 'acestep-cpp', model: 'balanced' }, undefined],
    [{ kind: 'native', engine: 'minimaxmusic-cpp', model: 'balanced' }, 'wav32'],
  ]);
});

it.each([
  { engine: 'acestep-cpp' as const, settings: { duration: 18, inference_steps: 7, guidance_scale: 2.5 }, extra: { ace: { guidance_scale: 2.5 } } },
  { engine: 'minimaxmusic-cpp' as const, settings: { duration: 18, steps: 7, lm_seed: 55 }, extra: { lm_seed: 55 } },
])('reuses the original profile and settings of a migrated $engine take', async ({ engine, settings, extra }) => {
  const song = { title: 'Old take', style: 'folk', lyrics: 'verse', ditModel: engine, lmModel: 'original', generationParams: { ...settings, seed: 42 } } as Song;
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ engine, engine_installed: true,
    profiles: [{ id: 'original', label: 'Original', ready: true, total_bytes: 1, missing: [] }] })));
  act(() => root.render(panel({ musicEngine: engine, initialData: { song, timestamp: 1 } })));
  await flush();
  expect(host.querySelector<HTMLSelectElement>('select[aria-label="Local model"]')!.value).toBe('original');
  click(buttonNamed('create')!);
  expect(sent.at(-1)).toMatchObject({ music_source: { kind: 'native', engine, model: 'original' },
    style: 'folk', lyrics: 'verse', duration_seconds: 18, steps: 7, seed: 42, ...extra });
});

it('asks for an explicit profile when a migrated ACE take used custom weights', async () => {
  const engine = 'acestep-cpp';
  const models = ['acestep-v15-xl-sft-Q4_K_M.gguf', 'acestep-5Hz-lm-4B-BF16.gguf', 'vae-BF16.gguf'];
  const song = { title: 'Old ACE take', style: 'synthwave', lyrics: 'verse', ditModel: engine, generationParams: {
    synth_model: models[0], lm_model: models[1], vae: models[2],
    duration: 120, inference_steps: 8, seed: 1234, guidance_scale: 2.5,
  } } as Song;
  localStorage.setItem('studio.nativeMusicProfiles', JSON.stringify({ selected: { [engine]: 'native' }, settings: {} }));
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ engine, engine_installed: true,
    profiles: [{ id: 'native', label: 'XL Turbo', ready: true, total_bytes: 1, missing: [] }] })));
  act(() => root.render(panel({ musicEngine: engine, initialData: { song, timestamp: 1 } })));
  await flush();

  const profile = host.querySelector<HTMLSelectElement>('select[aria-label="Local model"]')!;
  const generate = buttonNamed('create') as HTMLButtonElement;
  expect(profile.value).toBe('');
  expect(generate.disabled).toBe(true);
  expect(host.textContent).toContain('Choose a profile to create a new take.');
  for (const model of models) expect(host.textContent).toContain(model);
  const duration = [...host.querySelectorAll('label')].find(label => label.textContent === 'Duration (seconds)')!.querySelector('input')!;
  expect(duration.value).toBe('120');

  act(() => {
    profile.value = 'native';
    profile.dispatchEvent(new Event('change', { bubbles: true }));
  });
  expect(generate.disabled).toBe(false);
  expect(host.textContent).not.toContain(models[0]);
  click(generate);
  expect(sent.at(-1)).toMatchObject({
    music_source: { kind: 'native', engine, model: 'native' },
    style: 'synthwave', lyrics: 'verse', duration_seconds: 120, steps: 8, seed: 1234,
    ace: { guidance_scale: 2.5 },
  });
});

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

  it('reuses the saved cloud model and requires a new choice when that model disappears', async () => {
    configured = true;
    vi.mocked(fetch).mockImplementation(async url =>
      String(url) === '/v1/openrouter/catalog' ? catalog() : Response.json({}));
    const song = { ditModel: 'openrouter', title: 'Take', style: 'piano', lyrics: 'verse',
      generationParams: { model: 'test/music' } } as Song;
    const show = (song: Song, timestamp: number) => act(() => root.render(panel({
      musicEngine: 'openrouter', initialData: { song, timestamp },
    })));
    show(song, 1);
    await flush();
    expect(select('OpenRouter music model').value).toBe('test/music');
    expect(sent).toHaveLength(0);
    click(create());
    expect(sent[0]).toMatchObject({ style: 'piano', lyrics: 'verse', music_source: { model: 'test/music' } });
    show({ ...song, generationParams: { model: 'retired/model' } }, 2);
    await flush();
    expect(select('OpenRouter music model').value).toBe('');
    expect(create().disabled).toBe(true);
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
