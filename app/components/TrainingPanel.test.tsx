import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { TrainingPanel } from './TrainingPanel';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ t: (key: string) => key, songCount: String }),
}));

it.each([false, true])('uses the selected recipe after revisiting songs (automatic start: %s)', async automatic => {
  const state = {
    pack: [], pack_ready: true, card_trains: true, listen: { ready: true }, runs: [],
    recipe_defaults: { preset: 'balanced', seed: 42 },
    recipe_fields: [{ key: 'preset', kind: 'choice', group: 'stop', choices: ['fast', 'balanced'] }],
    datasets: [{ id: 'set', name: 'Test songs', items: [{
      id: 'track', source: 'track.wav', title: 'Track', seconds: 10, lyrics_state: 'done', style_state: 'done',
    }] }],
    prepare: automatic ? { dataset: 'set', finished: false, stages: [], pending: [], failures: [], notices: [] } : null,
  };
  const requests = new Map<string, unknown>();
  vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
    if (init?.body) requests.set(url, JSON.parse(String(init.body)));
    return Response.json(url === '/v1/training' ? state : {});
  });
  const host = document.createElement('div');
  const root = createRoot(host);
  const press = async (text: string) => {
    const target = Array.from(host.querySelectorAll('button')).find(button => button.textContent?.includes(text));
    expect(target?.disabled).toBe(false);
    await act(async () => target!.click());
  };
  try {
    await act(async () => root.render(<TrainingPanel />));
    await press('Test songs');
    await press('2 · trainingStepTrain');
    const preset = host.querySelector<HTMLSelectElement>('select:has(option[value="fast"])')!;
    await act(async () => {
      preset.value = 'fast';
      preset.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await press('1 · trainingStepSongs');
    const recipe = { preset: 'fast', seed: 42 };
    if (automatic) {
      const label = Array.from(host.querySelectorAll('label')).find(node => node.textContent?.includes('trainingTrainWhenReady'));
      await act(async () => label!.querySelector('input')!.click());
      expect(requests.get('/v1/training/prepare/train-after')).toEqual({ train: { name: 'Test songs', recipe } });
    } else {
      await press('2 · trainingStepTrain');
      expect(host.querySelector<HTMLSelectElement>('select:has(option[value="fast"])')?.value).toBe('fast');
      await press('trainingStart');
      expect(requests.get('/v1/training/runs')).toEqual({ dataset_id: 'set', name: 'Test songs', recipe });
    }
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
it.each(['sa3', 'heartmula'])('starts HOT-Step %s training with its native defaults', async target => {
  const state = {
    pack: [], pack_ready: false, card_trains: false, listen: { ready: true }, runs: [],
    recipe_defaults: { preset: 'balanced', seed: 42 }, recipe_fields: [],
    datasets: [{ id: 'instrumental-set', name: 'Instrumentals', items: [{
      id: 'track', source: 'track.wav', title: 'Track', seconds: 4, lyrics_state: 'instrumental',
      style_state: 'done', instrumental: target === 'sa3', style: 'Steady piano', lyrics: target === 'heartmula' ? 'We rise with the morning light' : '',
    }] }],
  };
  const start = vi.fn();
  vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
    if (url === '/v1/training') return Response.json(state);
    if (url === `/v1/training/hotstep-${target}` && init?.method === 'POST') {
      start(JSON.parse(String(init.body)));
      return Response.json({ id: 'run' });
    }
    if (url === `/v1/training/hotstep-${target}`) return Response.json({
      cuda: true, installable: true, ready: true, installing: false, missing_bytes: 0,
    });
    return Response.json({});
  });
  const host = document.createElement('div');
  const root = createRoot(host);
  const press = async (text: string) => {
    const button = [...host.querySelectorAll('button')].find(item => item.textContent?.includes(text));
    expect(button?.disabled).toBe(false);
    await act(async () => button!.click());
  };
  try {
    await act(async () => root.render(<TrainingPanel />));
    await press('Instrumentals');
    await press('2 · trainingStepTrain');
    const engine = host.querySelector<HTMLSelectElement>('select:has(option[value="sa3"])')!;
    await act(async () => {
      engine.value = target;
      engine.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await press('trainingStart');
    expect(start).toHaveBeenCalledWith({
      dataset_id: 'instrumental-set', name: 'Instrumentals',
      settings: { steps: 200, rank: 8, alpha: 8, learning_rate: 0.0001, duration_seconds: 30, seed: 42 },
    });
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
