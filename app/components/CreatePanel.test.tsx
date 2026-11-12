import React, { Activity, act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Song, YueRequest } from '../types';

const composeScore = vi.fn();
const instrumentalScore = vi.fn();

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({
  SELF_READ: {},
  libraryChanged: vi.fn(),
  useActivity: () => ({ data: [] }),
  useAssistantStatus: () => ({ data: null }),
  useSetupStatus: () => ({ isError: false, data: { ready: true, engine_ready: true } }),
}));
vi.mock('../services/mcpBridge', () => ({ useBridgeCommand: () => undefined }));
vi.mock('../services/transcription', () => ({ composeScore: (...args: unknown[]) => composeScore(...args), composePlans: vi.fn(), transcribe: vi.fn() }));
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
  localStorage.clear();
  composeScore.mockReset();
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

function panel(props: { initialData?: { song: Song; timestamp: number } | null; visible?: boolean }) {
  return (
    <Activity mode={props.visible === false ? 'hidden' : 'visible'}>
      <CreatePanel onGenerate={request => sent.push(request)} isGenerating={false} initialData={props.initialData ?? null}
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
