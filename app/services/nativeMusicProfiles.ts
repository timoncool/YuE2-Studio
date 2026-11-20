import { useEffect, useState } from 'react';
import type { MusicEngine, NativeMusicEngine, YueRequest } from '../types';

export const nativeEngines = { 'minimaxmusic-cpp': 'MiniMax Music3', 'acestep-cpp': 'ACE-Step' } as const;
export const isNativeMusicEngine = (value: unknown): value is NativeMusicEngine =>
  value === 'minimaxmusic-cpp' || value === 'acestep-cpp';
export const isMusicEngine = (value: unknown): value is MusicEngine =>
  value === 'local' || value === 'openrouter' || isNativeMusicEngine(value);

interface NativeProfile {
  id: string;
  label: string;
  total_bytes: number;
  ready: boolean;
  missing: string[];
}

interface NativeProfiles {
  engine: NativeMusicEngine;
  engine_installed: boolean;
  profiles: NativeProfile[];
}

const STORAGE = 'studio.nativeMusicProfiles';
type Control = 'duration_seconds' | 'steps' | 'seed' | 'lm_seed' | 'guidance_scale';
type Settings = Partial<Record<Control | 'output_format', string>>;
type Preferences = {
  selected: Partial<Record<NativeMusicEngine, string>>;
  settings: Record<string, Settings>;
};

function loadPreferences(): Preferences {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE) ?? 'null') as Partial<Preferences> | null;
    return { selected: saved?.selected ?? {}, settings: saved?.settings ?? {} };
  } catch {
    return { selected: {}, settings: {} };
  }
}

function optionalNumber(value?: string): number | undefined {
  if (typeof value !== 'string' || !value.trim()) return undefined;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

export function nativeGenerationSettings(engine: NativeMusicEngine, settings: Settings): Partial<YueRequest> {
  const duration = optionalNumber(settings.duration_seconds);
  const steps = optionalNumber(settings.steps);
  const seed = optionalNumber(settings.seed);
  const lmSeed = optionalNumber(settings.lm_seed);
  const guidance = optionalNumber(settings.guidance_scale);
  return {
    ...(duration === undefined ? {} : { duration_seconds: duration }),
    ...(steps === undefined ? {} : { steps }),
    ...(seed === undefined ? {} : { seed }),
    ...(engine === 'minimaxmusic-cpp' ? { output_format: miniMaxOutputFormat(settings.output_format) } : {}),
    ...(engine === 'minimaxmusic-cpp' && lmSeed !== undefined ? { lm_seed: lmSeed } : {}),
    ...(engine === 'acestep-cpp' && guidance !== undefined ? { ace: { guidance_scale: guidance } } : {}),
  };
}

export function miniMaxOutputFormat(value: unknown): NonNullable<YueRequest['output_format']> {
  return value === 'mp3' || value === 'wav32' ? value : 'flac';
}

export function useNativeMusicProfiles(engine: NativeMusicEngine | undefined) {
  const [preferences, setPreferences] = useState(loadPreferences);
  const [legacyAce, setLegacyAce] = useState<{ models: string[]; settings: Settings }>();
  const [refreshCount, setRefreshCount] = useState(0);
  const [response, setResponse] = useState<{ engine: NativeMusicEngine; data?: NativeProfiles; error?: boolean }>();
  useEffect(() => {
    if (!engine) return;
    const controller = new AbortController();
    setResponse(undefined);
    void (async () => {
      try {
        const reply = await fetch(`/v1/music/native/${engine}/profiles`, { signal: controller.signal });
        if (!reply.ok) throw new Error(`HTTP ${reply.status}`);
        const data = await reply.json() as NativeProfiles;
        if (data.engine !== engine || !Array.isArray(data.profiles)) throw new Error('Invalid native profile response');
        if (!controller.signal.aborted) setResponse({ engine, data });
      } catch {
        if (!controller.signal.aborted) setResponse({ engine, error: true });
      }
    })();
    return () => controller.abort();
  }, [engine, refreshCount]);
  const result = response?.engine === engine ? response : undefined;
  const profiles = result?.data?.profiles ?? [];
  const remembered = engine ? preferences.selected[engine] : undefined;
  const pending = engine === 'acestep-cpp' ? legacyAce : undefined;
  const profile = pending ? undefined : profiles.find(entry => entry.id === remembered);
  const pair = profile && engine ? JSON.stringify([engine, profile.id]) : undefined;
  const update = (next: Preferences) => {
    setPreferences(next);
    try { localStorage.setItem(STORAGE, JSON.stringify(next)); } catch { /* Keep this session's choice. */ }
  };
  return {
    profiles,
    profile,
    error: result?.error === true,
    loading: Boolean(engine && !result),
    installed: result?.data?.engine_installed === true,
    ready: result?.data?.engine_installed === true && profile?.ready === true,
    originalModels: pending?.models,
    settings: pending?.settings ?? (pair ? preferences.settings[pair] ?? {} : {}),
    refresh: () => { setResponse(undefined); setRefreshCount(count => count + 1); },
    restore: (source: NativeMusicEngine, id: string, saved: Record<string, unknown>) => {
      const controls: Settings = {};
      const number = (key: Control, raw: unknown) => {
        if (typeof raw === 'number' && Number.isFinite(raw)) controls[key] = String(raw);
      };
      number('duration_seconds', saved.duration_seconds ?? saved.duration);
      number('steps', saved.steps ?? saved.inference_steps);
      number('seed', saved.seed);
      if (source === 'minimaxmusic-cpp') {
        number('lm_seed', saved.lm_seed);
        controls.output_format = miniMaxOutputFormat(saved.output_format);
      }
      if (source === 'acestep-cpp') {
        number('guidance_scale', saved.guidance_scale);
        const models = [saved.synth_model, saved.lm_model, saved.vae]
          .filter((value): value is string => typeof value === 'string' && Boolean(value.trim()));
        if (!id && models.length) {
          setLegacyAce({ models, settings: controls });
          return;
        }
      }
      setLegacyAce(undefined);
      update({
        selected: { ...preferences.selected, [source]: id },
        settings: id ? { ...preferences.settings, [JSON.stringify([source, id])]: controls } : preferences.settings,
      });
    },
    select: (id: string) => {
      if (!engine || (pending && !id)) return;
      if (pending) setLegacyAce(undefined);
      update({
        selected: { ...preferences.selected, [engine]: id },
        settings: pending
          ? { ...preferences.settings, [JSON.stringify([engine, id])]: pending.settings }
          : preferences.settings,
      });
    },
    resetSettings: () => {
      if (pending) {
        setLegacyAce(undefined);
        return;
      }
      if (!pair || !preferences.settings[pair]) return;
      const settings = { ...preferences.settings };
      delete settings[pair];
      update({ ...preferences, settings });
    },
    set: (key: keyof Settings, value: string) => {
      if (pending) setLegacyAce({ ...pending, settings: { ...pending.settings, [key]: value } });
      else if (pair) update({ ...preferences, settings: { ...preferences.settings, [pair]: { ...preferences.settings[pair], [key]: value } } });
    },
  };
}
