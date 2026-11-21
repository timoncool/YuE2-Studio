import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
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

it('displays Burn training progress and keeps a saved checkpoint installable after Stop', async () => {
  const run = {
    id: 'burn-run', engine: 'rvc-burn', dataset_id: 'voice-set', dataset_name: 'Isolated vocals',
    name: 'My voice', trigger: '', status: 'running', stage: 'rvc-burn', stages: ['rvc-burn'],
    recipe: { preset: 'rvc-burn', steps: 12 },
    steps: [{ step: 2, loss: 48.5 }], checkpoints: [], installed: [],
    rvc_best_checkpoint: { step: 1, mel: 48.5 }, rvc_checkpoint_ready: false, rvc_voice_id: null as string | null,
  };
  const state = {
    pack: [], pack_ready: false, card_trains: true, listen: { ready: true }, recipe_defaults: {}, recipe_fields: [],
    datasets: [{ id: 'voice-set', name: 'Isolated vocals', trigger: '', items: [] }],
    runs: [run],
  };
  const actions: string[] = [];
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') {
      actions.push(url);
      if (url.endsWith('/cancel')) {
        run.status = 'cancelled';
        run.stage = '';
        run.rvc_checkpoint_ready = true;
        return Response.json({ cancelled: true });
      }
      if (url.endsWith('/install')) {
        run.rvc_voice_id = 'installed-voice';
        return Response.json({ id: run.rvc_voice_id });
      }
    }
    return Response.json(url === '/v1/training' ? state : {});
  });
  const host = document.createElement('div');
  const root = createRoot(host);
  const button = (text: string) => [...host.querySelectorAll('button')].find(item => item.textContent?.includes(text));
  const press = async (text: string) => {
    const target = button(text);
    expect(target?.disabled).toBe(false);
    await act(async () => target!.click());
  };
  try {
    const client = new QueryClient();
    await act(async () => root.render(<QueryClientProvider client={client}><TrainingPanel /></QueryClientProvider>));
    await press('Isolated vocals');
    expect(host.textContent).toContain('trainingStep 2');
    expect(host.textContent).toContain('Mel 48.500');
    await press('trainingStop');
    expect(actions).toContain('/v1/training/runs/burn-run/cancel');
    expect(host.textContent).toContain('trainingStatus_cancelled');
    expect(host.textContent).toContain('Mel 48.500');
    await press('Install voice');
    expect(actions).toContain('/v1/training/rvc-burn/burn-run/install');
    expect(button('Voice in catalogue')?.disabled).toBe(true);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});

it.each([undefined, 'ruach-rs-tw-2.0x0.75'])('starts a signed trainer with training base %s', async base => {
  const state = {
    pack: [], pack_ready: false, trainer_weights_ready: true, training_shared_weights_ready: true,
    training_bases: base ? [{ id: 'stock', label: 'Stock', bytes: 300, installed: false },
      { id: base, label: 'Ruach', bytes: 400, installed: true }] : undefined,
    card_trains: true, listen: { ready: true }, runs: [],
    recipe_defaults: { preset: 'balanced', seed: 42 }, recipe_fields: [],
    trainers: [{ id: 'music-train-bundled', label: 'Bundled', version: 'original', installed: false, bytes: 500 },
      { id: 'signed-trainer', label: 'Signed', version: 'revised', installed_version: 'revised', installed: true, bytes: 700 }],
    datasets: [{ id: 'set', name: 'Test vocals', items: [{
      id: 'track', source: 'track.wav', title: 'Track', seconds: 12, lyrics_state: 'done', style_state: 'done',
    }] }],
  };
  const start = vi.fn();
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
    if (url === '/v1/training/runs' && init?.method === 'POST') start(JSON.parse(String(init.body)));
    return Response.json(url === '/v1/training' ? state : { id: 'run' });
  });
  const host = document.createElement('div');
  const root = createRoot(host);
  const press = async (text: string) => {
    const target = [...host.querySelectorAll('button')].find(button => button.textContent?.includes(text))!;
    expect(target.disabled).toBe(false);
    await act(async () => target.click());
  };
  try {
    await act(async () => root.render(<TrainingPanel />));
    await press('Test vocals');
    await press('2 · trainingStepTrain');
    const trainer = host.querySelector<HTMLSelectElement>('select:has(option[value="signed-trainer"])')!;
    await act(async () => {
      trainer.value = 'signed-trainer';
      trainer.dispatchEvent(new Event('change', { bubbles: true }));
    });
    if (base) {
      const baseSelect = host.querySelector<HTMLSelectElement>(`select:has(option[value="${base}"])`)!;
      await act(async () => {
        baseSelect.value = base;
        baseSelect.dispatchEvent(new Event('change', { bubbles: true }));
      });
    }
    await press('trainingStart');
    expect(start).toHaveBeenCalledWith({ dataset_id: 'set', name: 'Test vocals', recipe: state.recipe_defaults,
      trainer_id: 'signed-trainer', ...(base ? { base_id: base } : {}) });
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
it.each(['sa3', 'heartmula', 'rvc'])('starts %s training with its native defaults', async target => {
  const endpoint = `/v1/training/${target === 'rvc' ? 'rvc-burn' : `hotstep-${target}`}`;
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
    if (url === endpoint && init?.method === 'POST') {
      start(JSON.parse(String(init.body)));
      return Response.json({ id: 'run' });
    }
    if (url === endpoint) return Response.json({
      cuda: true, installable: true, ready: true, installing: false, missing_bytes: 0, missing: [],
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
      settings: target === 'rvc' ? { epochs: 5, batch_size: 4, learning_rate: 0.0001 }
        : { steps: 200, rank: 8, alpha: 8, learning_rate: 0.0001, duration_seconds: 30, seed: 42 },
    });
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
