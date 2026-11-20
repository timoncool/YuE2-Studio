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
type Settings = Partial<Record<Control, string>>;
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
    ...(engine === 'minimaxmusic-cpp' && lmSeed !== undefined ? { lm_seed: lmSeed } : {}),
    ...(engine === 'acestep-cpp' && guidance !== undefined ? { ace: { guidance_scale: guidance } } : {}),
  };
}

export function useNativeMusicProfiles(engine: NativeMusicEngine | undefined) {
  const [preferences, setPreferences] = useState(loadPreferences);
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
  const profile = profiles.find(entry => entry.id === remembered);
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
    settings: pair ? preferences.settings[pair] ?? {} : {},
    refresh: () => { setResponse(undefined); setRefreshCount(count => count + 1); },
    select: (id: string) => {
      if (engine) update({ ...preferences, selected: { ...preferences.selected, [engine]: id } });
    },
    set: (key: Control, value: string) => {
      if (pair) update({ ...preferences, settings: { ...preferences.settings, [pair]: { ...preferences.settings[pair], [key]: value } } });
    },
  };
}
