import React, { Activity, act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Song, YueRequest } from '../types';
import { useGenerations } from '../services/generations';
import { fetchAdapters } from '../services/adapters';
import { afterGenerationPause } from '../services/generationPause';

const bridgeCommands = new Map<string, (args: Record<string, unknown>) => unknown>();
const composeScore = vi.fn();
const transcribe = vi.fn();
const instrumentalScore = vi.fn();
let ready = true;
let enginePaused = false;
let configured = false;
let librarySongs: Song[] = [];
vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../services/studioQueries', async original => ({
  ...await original<typeof import('../services/studioQueries')>(),
  useLibrarySongs: () => ({ data: librarySongs }),
  SELF_READ: {},
  libraryChanged: vi.fn(),
  useActivity: () => ({ data: [] }),
  useAssistantStatus: () => ({ data: null }),
  setupStatusChanged: vi.fn(),
  useSetupStatus: () => ({ isError: false, data: { ready, engine_ready: ready && !enginePaused, engine_paused: enginePaused } }),
  useOpenRouterSettings: () => ({ data: { configured } }),
}));
vi.mock('../services/mcpBridge', () => ({
  useBridgeCommand: (name: string, handler: (args: Record<string, unknown>) => unknown) => bridgeCommands.set(name, handler),
}));
vi.mock('../services/transcription', () => ({ composeScore: (...args: unknown[]) => composeScore(...args), composePlans: vi.fn(), transcribe: (...args: unknown[]) => transcribe(...args) }));
vi.mock('../services/scoreApi', () => ({
  instrumentalScore: (abc: string) => instrumentalScore(abc),
  failed: (answer: { error?: string }) => typeof answer?.error === 'string',
  chordBed: vi.fn(),
  matchSections: vi.fn(),
}));
vi.mock('../services/adapters', async (original) => ({
  ...(await original<typeof import('../services/adapters')>()),
  fetchAdapters: vi.fn(async () => ({ installed: [{ id: 'instrumental', origin: { type: 'catalog', catalog_id: 'yue2-instrumental' }, slots: ['ar', 'nar'], scales: {}, error: null }], installing: [], slots: [] })),
  installCatalogAdapters: vi.fn(),
  useAdapterLibrary: () => ({ installed: [], slots: [] }),
}));
vi.mock('./AdapterPicker', () => ({ AdapterPicker: () => null }));
vi.mock('./AudioWaveform', () => ({ AudioWaveform: () => null }));
vi.mock('./ScoreView', () => ({ ScoreView: () => null }));
vi.mock('./WindowAudioCapture', () => ({
  WindowAudioCapture: ({ disabled, onRecording }: { disabled: boolean; onRecording: (file: File) => void }) =>
    <button disabled={disabled} onClick={() => onRecording(new File(['RIFF'], 'window-audio.wav', { type: 'audio/wav' }))}>Finish fixture recording</button>,
}));
vi.mock('./midi/MidiEditor', () => ({ MidiEditor: () => null }));
vi.mock('./MidiImportDialog', () => ({ MidiImportDialog: () => null }));
vi.mock('./PlaylistModals', () => ({ CreatePlaylistModal: () => null }));
vi.mock('./SlideToEnable', () => ({
  SlideToEnable: ({ on, onChange, offLabel, stopLabel }: { on: boolean; onChange: (on: boolean) => void; offLabel: string; stopLabel: string }) =>
    <button onClick={() => onChange(!on)}>{on ? stopLabel : offLabel}</button>,
}));

const { CreatePanel } = await import('./CreatePanel');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
const sent: YueRequest[] = [];

beforeEach(() => {
  sent.length = 0;
  bridgeCommands.clear();
  ready = true;
  enginePaused = false;
  configured = false;
  librarySongs = [];
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

function panel(props: Pick<React.ComponentProps<typeof CreatePanel>, 'musicEngine' | 'onMusicEngineChange' | 'activeJobCount' | 'queueBusy'> & { initialData?: { song: Song; timestamp: number } | null; visible?: boolean; request?: React.ComponentProps<typeof CreatePanel>['request']; onGenerate?: (request: YueRequest) => void }) {
  return (
    <Activity mode={props.visible === false ? 'hidden' : 'visible'}>
      <CreatePanel musicEngine={props.musicEngine} onMusicEngineChange={props.onMusicEngineChange} onGenerate={props.onGenerate ?? (request => sent.push(request))} isGenerating={Boolean(props.activeJobCount)} queueBusy={props.queueBusy} activeJobCount={props.activeJobCount} request={props.request} initialData={props.initialData ?? null}
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

it('uses explicit controls for the YuE2 engine and follows the shared paused state', async () => {
  act(() => root.render(panel({})));
  click(buttonNamed('Pause YuE2 engine')!);
  await flush();
  expect(fetch).toHaveBeenCalledWith('/engine/pause', { method: 'POST' });

  enginePaused = true;
  act(() => root.render(panel({})));
  expect(host.textContent).toContain('YuE2 engine paused');
  click(buttonNamed('Resume YuE2 engine')!);
  await flush();
  expect(fetch).toHaveBeenCalledWith('/engine/resume', { method: 'POST' });

  act(() => root.render(panel({ musicEngine: 'openrouter' })));
  expect(buttonNamed('Resume YuE2 engine')).toBeUndefined();
});

describe('pauses between repeated generations (#261)', () => {
  const renderQueue = (queueBusy: boolean, activeJobCount = 0) =>
    act(() => root.render(panel({ queueBusy, activeJobCount })));
  const choosePause = (value: string) => act(() => {
    const select = [...host.querySelectorAll('select')].find(element => element.closest('label')?.textContent?.includes('Pause between generations'))!;
    select.value = value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const advance = (ms: number) => act(() => { vi.advanceTimersByTime(ms); });

  beforeEach(async () => {
    renderQueue(true, 2);
    await flush();
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] });
    type(field('textarea')!, 'calm folk');
    click(buttonNamed('generateForeverSlide')!);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('keeps the original no-pause behavior by default', () => {
    click(buttonNamed('create')!);
    expect(sent).toHaveLength(1);
    renderQueue(false);
    expect(sent).toHaveLength(2);
  });

  it('counts a fixed interval only after the queue is actually empty', () => {
    choosePause('fixed');
    click(buttonNamed('create')!);
    renderQueue(true);
    advance(5 * 60_000);
    expect(sent).toHaveLength(1);
    renderQueue(false);
    advance(59_999);
    expect(sent).toHaveLength(1);
    advance(1);
    expect(sent).toHaveLength(2);
    expect(sent[1].style).toBe(sent[0].style);
  });

  it('samples a random interval and clears a pending repeat on Cancel all', () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.5);
    choosePause('random');
    click(buttonNamed('create')!);
    renderQueue(false);
    advance(119_999);
    expect(sent).toHaveLength(1);
    advance(1);
    expect(sent).toHaveLength(2);

    renderQueue(true);
    renderQueue(false);
    act(() => window.dispatchEvent(new CustomEvent('yue:cancel-all')));
    advance(180_000);
    expect(sent).toHaveLength(2);
  });

  it('does not turn a longer-than-browser-timeout pause into an immediate repeat', () => {
    const callback = vi.fn();
    const cancel = afterGenerationPause(2_147_483_647 + 1000, callback);
    advance(2_147_483_647);
    expect(callback).not.toHaveBeenCalled();
    advance(999);
    expect(callback).not.toHaveBeenCalled();
    advance(1);
    expect(callback).toHaveBeenCalledTimes(1);
    cancel();
  });

  it('shows the remaining pause and cancels it from the countdown', () => {
    choosePause('fixed');
    click(buttonNamed('create')!);
    expect(host.querySelector('[role="timer"]')).toBeNull();
    renderQueue(false);
    expect(host.querySelector('[role="timer"]')?.textContent).toBe('Next generation in 1:00');
    advance(1_000);
    expect(host.querySelector('[role="timer"]')?.textContent).toBe('Next generation in 0:59');

    click(buttonNamed('Cancel next generation')!);
    expect(host.querySelector('[role="timer"]')).toBeNull();
    expect(buttonNamed('Cancel next generation')).toBeUndefined();
    advance(60_000);
    expect(sent).toHaveLength(1);
  });

  it('clears a pending repeat when Without stopping is switched off', () => {
    choosePause('fixed');
    click(buttonNamed('create')!);
    renderQueue(false);
    click(buttonNamed('generateForeverStop')!);
    advance(60_000);
    expect(sent).toHaveLength(1);
  });
});

it('reads a captured window WAV through the existing cover transcription and score form', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ catalog: { transcriber: true } })));
  act(() => root.render(panel({})));
  await flush();
  click(buttonNamed('coverMode')!);
  await flush();
  const record = buttonNamed('Finish fixture recording')!;
  expect(record.disabled).toBe(false);
  click(record);
  await flush();
  expect(transcribe).toHaveBeenCalledWith({ file: expect.objectContaining({ name: 'window-audio.wav', type: 'audio/wav' }) },
    expect.any(Boolean), expect.any(AbortSignal));
  expect(field('textarea[aria-label="scoreCardTitle"]')?.value).toContain('X:1');
});

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

it('chooses MiniMax Music3 through audio.cpp and reuses the saved seed and format', async () => {
  let ready = false;
  let heartMuLaReady = false;
  let engineInstalled: boolean | undefined;
  const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/v1/music/audio-cpp/models/install') {
      expect(init?.method).toBe('POST');
      expect(JSON.parse(String(init?.body))).toEqual({ model: 'heartmula' });
      heartMuLaReady = true;
      return Response.json({ started: true, installing: true });
    }
    return Response.json(url === '/v1/music/audio-cpp/models' ? {
    engine: 'audio.cpp',
    engine_installed: engineInstalled,
    models: [{ id: 'minimax_music3', profile_id: 'quality-q8', quantization: 'Q8_0', ready,
      missing: ready ? [] : ['language_model_q8_0.gguf'] },
      { id: 'heartmula', name: 'HeartMuLa', profile_id: 'heartmula_q8_0', quantization: 'Q8_0',
        installable: true, bytes: 7659762592, ready: heartMuLaReady, missing: heartMuLaReady ? [] : ['heartmula.gguf'] }],
    } : {});
  });
  vi.stubGlobal('fetch', fetcher);
  function Editor() {
    const [engine, setEngine] = React.useState<NonNullable<React.ComponentProps<typeof CreatePanel>['musicEngine']>>('local');
    return panel({ musicEngine: engine, onMusicEngineChange: setEngine });
  }
  act(() => root.render(<Editor />));
  await flush();
  const option = host.querySelector<HTMLOptionElement>('option[value="audio.cpp"]')!;
  expect(option.textContent).toBe('audio.cpp');
  const engineSelect = option.parentElement as HTMLSelectElement;
  const choose = (select: HTMLSelectElement, value: string) => act(() => {
    select.value = value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
  choose(engineSelect, 'audio.cpp');
  await flush();
  const generate = () => buttonNamed('create') as HTMLButtonElement;
  expect(generate().disabled).toBe(true);
  expect(host.textContent).toContain('language_model_q8_0.gguf');
  ready = true;
  click(buttonNamed('Refresh models')!);
  await flush();
  expect(generate().disabled).toBe(false);
  engineInstalled = false;
  click(buttonNamed('Refresh models')!);
  await flush();
  expect(generate().disabled).toBe(true);
  engineInstalled = true;
  click(buttonNamed('Refresh models')!);
  await flush();
  expect(generate().disabled).toBe(false);
  expect(host.textContent).toContain('quality-q8 · Q8_0');
  const models = host.querySelector<HTMLOptionElement>('option[value="heartmula"]')!.parentElement as HTMLSelectElement;
  choose(models, 'heartmula');
  expect(models.value).toBe('heartmula');
  expect(generate().disabled).toBe(true);
  expect(host.textContent).toContain('heartmula.gguf');
  const installHeartMuLa = () => [...host.querySelectorAll('button')]
    .find(button => button.textContent?.startsWith('Install HeartMuLa'));
  expect(installHeartMuLa()?.textContent).toContain('7.13 GiB');
  click(installHeartMuLa()!);
  await flush();
  await flush();
  expect(heartMuLaReady).toBe(true);
  expect(installHeartMuLa()).toBeUndefined();

  const song = { title: 'Music3 take', style: 'folk', lyrics: '[verse] Good morning',
    ditModel: 'audio.cpp', lmModel: 'quality-q8', nativeReplayAvailable: false,
    generationParams: { music_source: { kind: 'audio_cpp', model: 'minimax_music3' },
      seed: 424242, duration_seconds: 18, steps: 7, guidance_scale: 2.5, output_format: 'mp3' },
  } as Song;
  act(() => root.render(panel({ musicEngine: 'audio.cpp', initialData: { song, timestamp: 1 } })));
  await flush();
  const label = (name: string) => [...host.querySelectorAll('label')].find(node => node.textContent?.startsWith(name))!;
  expect(label('Seed').querySelector('input')?.value).toBe('424242');
  expect(label('outputFormat').querySelector('select')?.value).toBe('mp3');
  expect(host.textContent).not.toContain('Language model seed');
  click(generate());
  expect(sent.at(-1)).toEqual({
    music_source: { kind: 'audio_cpp', model: 'minimax_music3' }, style: 'folk',
    lyrics: '[verse] Good morning', title: 'Music3 take',
    seed: 424242, duration_seconds: 18, steps: 7, cfg_scale: 2.5, output_format: 'mp3',
  });
});

it.each([
  ['heartmula', { heartmula: { top_k: 30, temperature: 0.9 } }, 'pop', ''],
  ['vevo2', { vevo2: { voice_song_id: 'original-take' } }, '', 'Words to sing'],
  ['midashenglm_gen', { midasheng: { caption: '', music: 'quiet piano', speech_text: '', speech_style: '', effects: '', environment: 'rain' } }, 'quiet piano', ''],
] as const)('restores and submits model-specific settings for %s', async (model, options, style, lyrics) => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ engine: 'audio.cpp', models: [
    { id: model, name: model, profile_id: 'q8', quantization: 'Q8_0', ready: true, missing: [] },
  ] })));
  const song = {
    title: 'Saved take', ditModel: 'audio.cpp', style, lyrics,
    generationParams: { music_source: { kind: 'audio_cpp', model }, ...options },
  } as Song;
  act(() => root.render(panel({ musicEngine: 'audio.cpp', initialData: { song, timestamp: 1 } })));
  await flush();
  click(buttonNamed('create')!);
  expect(sent.at(-1)).toMatchObject({ music_source: { kind: 'audio_cpp', model }, ...options });
});
it('restores a Magenta reference and submits either the available reference or text style', async () => {
  const model = 'magenta_realtime_2';
  librarySongs = [{ id: 'ref-take', title: 'Reference take', audioUrl: '/audio.wav' } as Song];
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ engine: 'audio.cpp', models: [
    { id: model, name: 'Magenta RealTime 2', profile_id: 'small-f16', quantization: 'F16', ready: true, missing: [] },
  ] })));
  const song = {
    title: 'Saved instrumental', ditModel: 'audio.cpp', style: 'jazz piano', lyrics: 'old lyrics',
    generationParams: { music_source: { kind: 'audio_cpp', model }, magenta: { reference_song_id: 'ref-take' } },
  } as Song;
  act(() => root.render(panel({ musicEngine: 'audio.cpp', initialData: { song, timestamp: 1 } })));
  await flush();
  const generate = buttonNamed('create')!;
  expect(field('textarea[aria-label="lyrics"]')).toBeNull();
  expect(field('textarea[aria-label="styleCardTitle"]')).toBeNull();
  click(generate);
  expect(sent.at(-1)).toMatchObject({
    music_source: { kind: 'audio_cpp', model }, magenta: { reference_song_id: 'ref-take' }, lyrics: '',
  });

  const reference = [...host.querySelectorAll('select')].find(select => select.value === 'ref-take')!;
  act(() => {
    reference.value = '';
    reference.dispatchEvent(new Event('change', { bubbles: true }));
  });
  expect(field('textarea[aria-label="styleCardTitle"]')!.value).toBe('jazz piano');
  click(generate);
  expect(sent.at(-1)).toMatchObject({ music_source: { kind: 'audio_cpp', model }, style: 'jazz piano', magenta: {}, lyrics: '' });

  librarySongs = [];
  act(() => root.render(panel({ musicEngine: 'audio.cpp', initialData: { song, timestamp: 2 } })));
  await flush();
  expect(generate.disabled).toBe(true);
  expect(reference.selectedOptions[0].disabled).toBe(true);
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

  act(() => root.render(panel({ musicEngine: engine, initialData: { song, timestamp: 2 } })));
  await flush();
  expect(profile.value).toBe('');
  expect(host.textContent).toContain(models[0]);
  click(host.querySelector('button[aria-label="resetCreateForm"]')!);
  expect(profile.value).toBe('native');
  expect(host.textContent).not.toContain(models[0]);
  expect(duration.value).toBe('120');
  expect(field('textarea[aria-label="lyrics"]')!.value).toBe('');
  expect(sent).toHaveLength(1);
});

describe('an instrumental (#217)', () => {
  it('waits for the Instrumental LoRA and permits an ordinary request without it (#234)', async () => {
    const installed = await fetchAdapters();
    let finishLookup!: (result: typeof installed) => void;
    vi.mocked(fetchAdapters).mockReturnValueOnce(new Promise(resolve => { finishLookup = resolve; }));
    act(() => root.render(panel({})));
    await flush();
    type(field('textarea')!, 'calm folk, acoustic guitar');
    click(buttonNamed('cotOff')!);
    const instrumental = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    click(instrumental);
    click(buttonNamed('createInstrumental')!);
    await flush();
    expect(sent).toHaveLength(0);

    await act(async () => { finishLookup(installed); });
    click(buttonNamed('createInstrumental')!);
    await flush();
    expect(sent[0].adapters).toEqual([expect.objectContaining({ id: 'instrumental' })]);
    expect(sent[0].lyrics).toBe('');

    click(instrumental);
    click(buttonNamed('create')!);
    await flush();
    expect(sent).toHaveLength(2);
    expect(sent[1].adapters).toBeUndefined();
  });

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
  const sourceButton = () => host.querySelector<HTMLButtonElement>('button[title="recogniseFromSourceHint"]')!;
  const lyricsBox = () => field('textarea[aria-label="lyrics"]')!;
  const requests = () => vi.mocked(fetch).mock.calls
    .filter(([url]) => url === '/v1/lyrics/recognise')
    .map(([, options]) => JSON.parse(options!.body as string));
  const respond = (reply: (body: RequestInit) => Promise<Response>) =>
    vi.mocked(fetch).mockImplementation(async (url, options) =>
      url === '/v1/lyrics/recognise' ? reply(options!) : Response.json({}));
  const cover = async (id: number, songId = 'cover') => {
    const song = { id: songId, title: songId, lyrics: '' } as Song;
    act(() => root.render(panel({ request: { id, kind: 'transcribe', song, melodyOnly: true } })));
    await flush();
  };
  const confirm = () => click([...host.querySelectorAll('button')].find(
    button => button.textContent === 'recogniseFromSource' && button !== sourceButton(),
  )!);

  it('asks before replacing text, allows refusal, and reports a recogniser refusal', async () => {
    await cover(7);
    type(lyricsBox(), 'Words already in the form');
    respond(async () => Response.json({ error: 'karaoke.no-recogniser' }, { status: 409 }));
    click(sourceButton());
    expect(requests()).toEqual([]);
    click([...host.querySelectorAll('button')].find(button => button.textContent === 'cancel')!);
    expect(requests()).toEqual([]);
    expect(lyricsBox().value).toBe('Words already in the form');

    click(sourceButton());
    confirm();
    await flush();
    expect(requests()).toEqual([{ song_id: 'cover' }]);
    expect(lyricsBox().value).toBe('Words already in the form');
    expect(host.textContent).toContain('karaokeNoRecogniser');

    respond(async () => Response.json({ text: 'Words from the library recording' }));
    click(sourceButton());
    confirm();
    await flush();
    expect(lyricsBox().value).toBe('Words from the library recording');
  });

  it('dismisses a pending replacement confirmation when the source changes', async () => {
    await cover(12, 'first');
    type(lyricsBox(), 'Words already in the form');
    click(sourceButton());
    expect(host.textContent).toContain('recogniseFromSourceReplace');

    await cover(13, 'second');
    expect(host.textContent).not.toContain('recogniseFromSourceReplace');
    expect(requests()).toEqual([]);
  });

  it('sends the chosen audio file instead of the previous library track', async () => {
    await cover(8);
    const file = new File(['RIFF'], 'singing.wav', { type: 'audio/wav' });
    const input = host.querySelector<HTMLInputElement>('input[type="file"][accept*=".wav"]')!;
    act(() => {
      Object.defineProperty(input, 'files', { value: [file], configurable: true });
      input.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await flush();
    respond(async () => Response.json({ text: 'Words from the file' }));
    click(sourceButton());
    await flush();
    expect(requests()).toEqual([{ audio_base64: 'UklGRg==', audio_format: 'wav', audio_name: 'singing.wav' }]);
    expect(lyricsBox().value).toBe('Words from the file');
  });

  it('recognises a new source while the abandoned request finishes later', async () => {
    const replies: ((response: Response) => void)[] = [];
    respond(async () => new Promise(resolve => replies.push(resolve)));
    await cover(9, 'first');
    click(sourceButton());
    expect(sourceButton().disabled).toBe(true);

    await cover(10, 'second');
    expect(sourceButton().disabled).toBe(false);
    click(sourceButton());
    expect(requests()).toEqual([{ song_id: 'first' }, { song_id: 'second' }]);
    await act(async () => replies[0](Response.json({ error: 'karaoke.no-recogniser' }, { status: 409 })));
    expect(host.textContent).not.toContain('karaokeNoRecogniser');
    expect(sourceButton().disabled).toBe(true);
    await act(async () => replies[1](Response.json({ text: 'Second recording' })));
    expect(lyricsBox().value).toBe('Second recording');
  });

  it('keeps text edited while recognition is running', async () => {
    await cover(11);
    let finish!: (reply: Response) => void;
    respond(async () => new Promise(resolve => { finish = resolve; }));
    click(sourceButton());
    expect(requests()).toEqual([{ song_id: 'cover' }]);
    type(lyricsBox(), 'my newer lyrics');
    expect(sourceButton().disabled).toBe(false);
    await act(async () => finish(Response.json({ text: 'older recognition result' })));
    expect(lyricsBox().value).toBe('my newer lyrics');
    expect(sourceButton().disabled).toBe(false);
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

it('reuses the MiniMax-H3 prompt, duration, seed and format without a lyrics field', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ engine: 'audio.cpp', models: [
    { id: 'minimax_h3', name: 'MiniMax-H3', profile_id: 'minimax_h3_q4_k', quantization: 'Q4_K', ready: true, missing: [] },
  ] })));
  const song = {
    title: 'H3 take', ditModel: 'audio.cpp', style: 'funk groove', lyrics: 'Retained draft lyrics',
    generationParams: {
      music_source: { kind: 'audio_cpp', model: 'minimax_h3' },
      duration_seconds: 4, steps: 20, seed: 2026, guidance_scale: 1, output_format: 'wav32',
    },
  } as Song;
  act(() => root.render(panel({ musicEngine: 'audio.cpp', initialData: { song, timestamp: 1 } })));
  await flush();
  expect(field('textarea[aria-label="lyrics"]')).toBeNull();
  expect(field('input[max="20"]')?.value).toBe('4');
  click(buttonNamed('create')!);
  expect(sent.at(-1)).toMatchObject({
    music_source: { kind: 'audio_cpp', model: 'minimax_h3' },
    style: 'funk groove', lyrics: '', duration_seconds: 4, steps: 20, seed: 2026,
    cfg_scale: 1, output_format: 'wav32',
  });
});

it('reuses a Stable Audio 3 text prompt and its sampler and guidance controls', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ engine: 'audio.cpp', models: [
    { id: 'stable_audio', name: 'Stable Audio 3', profile_id: 'stable_audio_3_small_music_q8_0',
      quantization: 'Q8_0', ready: true, missing: [] },
  ] })));
  const stable_audio = { negative_prompt: 'vocals', sampler_mode: 'euler', apg_scale: 0.5, duration_padding_sec: 1 };
  const song = {
    title: 'Stable take', ditModel: 'audio.cpp', style: 'instrumental piano', lyrics: 'Previous draft',
    generationParams: {
      music_source: { kind: 'audio_cpp', model: 'stable_audio' },
      duration_seconds: 4, steps: 4, seed: 42, guidance_scale: 2, output_format: 'wav32', stable_audio,
    },
  } as Song;
  act(() => root.render(panel({ musicEngine: 'audio.cpp', initialData: { song, timestamp: 1 } })));
  await flush();
  expect(field('textarea[aria-label="lyrics"]')).toBeNull();
  const sampler = [...host.querySelectorAll('label')].find(node => node.textContent?.startsWith('Sampler'));
  expect(sampler?.querySelector('select')?.value).toBe('euler');
  click(buttonNamed('create')!);
  expect(sent.at(-1)).toMatchObject({
    music_source: { kind: 'audio_cpp', model: 'stable_audio' },
    style: 'instrumental piano', lyrics: '', duration_seconds: 4, steps: 4, seed: 42,
    cfg_scale: 2, output_format: 'wav32', stable_audio,
  });
});

it('submits a MuLaCover library original and restores it for a new take', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ engine: 'audio.cpp', models: [
    { id: 'mulacover', name: 'MuLaCover', profile_id: 'mulacover_q8_0', quantization: 'Q8_0', ready: true, missing: [] },
  ] })));
  const song = {
    title: 'Cover take', ditModel: 'audio.cpp', style: 'warm piano', lyrics: '[verse] Morning',
    generationParams: {
      music_source: { kind: 'audio_cpp', model: 'mulacover' },
      cover_of: 'original-track', duration_seconds: 3, seed: 2026, output_format: 'flac',
    },
  } as Song;
  act(() => root.render(panel({ musicEngine: 'audio.cpp', initialData: { song, timestamp: 1 } })));
  await flush();
  const picker = [...host.querySelectorAll('select')].find(select => select.closest('label')?.textContent?.includes('Original song'));
  expect(picker?.value).toBe('original-track');
  click(buttonNamed('create')!);
  expect(sent.at(-1)).toMatchObject({
    music_source: { kind: 'audio_cpp', model: 'mulacover' }, cover_of: 'original-track',
    style: 'warm piano', lyrics: '[verse] Morning', duration_seconds: 3, seed: 2026, output_format: 'flac',
  });
});

describe('Auto max duration', () => {
  const autoButton = () => buttonNamed('maxDurationAuto');
  // an instrumental with No score goes straight to onGenerate, with no score to compose first
  const createOne = async () => {
    click(buttonNamed('cotOff')!);
    click(buttonNamed('createInstrumental')!);
    await flush();
  };
  const startInstrumental = async () => {
    act(() => root.render(panel({})));
    await flush();
    type(field('textarea')!, 'calm folk, acoustic guitar');
    click(host.querySelector<HTMLInputElement>('input[type="checkbox"]')!);
    await flush();
  };

  it('is where a new studio starts, and sends no ceiling: the song ends where the model ends it', async () => {
    await startInstrumental();
    expect(autoButton()?.getAttribute('aria-pressed')).toBe('true');
    await createOne();
    expect(sent).toHaveLength(1);
    expect(sent[0].duration_seconds).toBeUndefined();
  });

  it('switched off sends the seconds again and is kept for the next song', async () => {
    await startInstrumental();
    click(autoButton()!);
    await flush();
    expect(autoButton()?.getAttribute('aria-pressed')).toBe('false');
    await createOne();
    expect(sent[0].duration_seconds).toBe(130);
    expect(localStorage.getItem('studio.createDuration')).toBe('130');
    click(autoButton()!);
    await flush();
    expect(localStorage.getItem('studio.createDuration')).toBe('auto');
  });

  it('keeps seconds a user chose before Auto existed', async () => {
    localStorage.setItem('studio.createDuration', '240');
    await startInstrumental();
    expect(autoButton()?.getAttribute('aria-pressed')).toBe('false');
    await createOne();
    expect(sent[0].duration_seconds).toBe(240);
    click(autoButton()!);
    click(autoButton()!);
    await createOne();
    expect(sent[1].duration_seconds).toBe(240);
  });

  it('sends the manual ceiling or omits it through the Create MCP bridge', async () => {
    act(() => root.render(panel({})));
    await flush();
    const command = (name: string, args: Record<string, unknown> = {}) => bridgeCommands.get(name)!(args);
    const read = () => command('create_get') as { fields: Record<string, unknown>; request: YueRequest };

    act(() => { command('create_set', { fields: { style: 'calm acoustic folk, gentle guitar instrumental', cot: 'off', duration_seconds: 10 } }); });
    expect(read().fields.duration_seconds).toBe('10');
    expect(read().request.duration_seconds).toBe(10);
    act(() => { command('create_submit'); });
    await flush();

    act(() => { command('create_set', { fields: { duration_seconds: 'auto' } }); });
    expect(read().fields.duration_seconds).toBe('auto');
    expect(read().request).not.toHaveProperty('duration_seconds');
    act(() => { command('create_submit'); });
    await flush();
    expect(sent).toHaveLength(2);
    expect(sent[0].duration_seconds).toBe(10);
    expect(sent[1]).not.toHaveProperty('duration_seconds');
  });
});

it('restores HOT-Step cover source and lead sheet from a saved take', async () => {
  vi.stubGlobal('fetch', vi.fn(async (url: string) => Response.json(url === '/v1/music/native/hotstep/profiles' ? {
    engine: 'hotstep', engine_installed: true,
    profiles: [{ id: 'heartmula-mulacover', label: 'MuLaCover · HOT-Step · BF16', ready: true, missing: [] }],
  } : {})));
  const abc = 'X:1\nQ:1/4=120\nK:C\nV:Vocal\n|"C"C E G c|';
  const song = {
    title: 'HOT-Step cover', ditModel: 'hotstep', style: 'folk', lyrics: '[Verse] Morning',
    generationParams: {
      music_source: { kind: 'native', engine: 'hotstep', model: 'heartmula-mulacover' },
      cover_of: 'recording-original', abc, duration_seconds: 4, seed: 42, steps: 10, guidance_scale: 1.5, output_format: 'flac',
    },
  } as Song;
  act(() => root.render(panel({ musicEngine: 'hotstep', initialData: { song, timestamp: 1 } })));
  await flush();
  const score = [...host.querySelectorAll('textarea')].find(field => field.closest('label')?.textContent?.includes('Cover score'));
  expect(score?.value).toBe(abc);
  click(buttonNamed('create')!);
  expect(sent.at(-1)).toMatchObject({
    music_source: { kind: 'native', engine: 'hotstep', model: 'heartmula-mulacover' }, cover_of: 'recording-original',
    abc, duration_seconds: 4, steps: 10, seed: 42, cfg_scale: 1.5, output_format: 'flac',
  });
});
