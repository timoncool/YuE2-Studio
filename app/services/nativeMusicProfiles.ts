import { useEffect, useState } from 'react';
import type { MusicEngine, NativeMusicEngine, ProfileMusicEngine, YueRequest } from '../types';
import { apiUrl } from './apiBase';

export const nativeEngines = { 'minimaxmusic-cpp': 'MiniMax Music3', 'acestep-cpp': 'ACE-Step', hotstep: 'HOT-Step' } as const;
export const profileMusicEngines = { ...nativeEngines, 'audio.cpp': 'audio.cpp' } as const;
export const audioCppMiniMax = 'minimax_music3';
export const isNativeMusicEngine = (value: unknown): value is NativeMusicEngine =>
  value === 'minimaxmusic-cpp' || value === 'acestep-cpp' || value === 'hotstep';
export const isProfileMusicEngine = (value: unknown): value is ProfileMusicEngine =>
  value === 'audio.cpp' || isNativeMusicEngine(value);
export const isMusicEngine = (value: unknown): value is MusicEngine =>
  value === 'local' || value === 'openrouter' || isProfileMusicEngine(value);

interface NativeProfile {
  id: string;
  label: string;
  total_bytes: number;
  missing_bytes?: number;
  adapters?: string[];
  installable?: boolean;
  license_url?: string;
  ready: boolean;
  missing: string[];
}

interface NativeProfiles {
  engine: ProfileMusicEngine;
  engine_installed: boolean;
  profiles: NativeProfile[];
}

const STORAGE = 'studio.nativeMusicProfiles';
type Control = 'duration_seconds' | 'steps' | 'seed' | 'lm_seed' | 'guidance_scale';
type Settings = Partial<Record<Control | 'output_format', string>>;
type Preferences = {
  selected: Partial<Record<ProfileMusicEngine, string>>;
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

export function nativeGenerationSettings(engine: ProfileMusicEngine, settings: Settings): Partial<YueRequest> {
  const duration = optionalNumber(settings.duration_seconds);
  const steps = optionalNumber(settings.steps);
  const seed = optionalNumber(settings.seed);
  const lmSeed = optionalNumber(settings.lm_seed);
  const guidance = optionalNumber(settings.guidance_scale);
  return {
    ...(duration === undefined ? {} : { duration_seconds: duration }),
    ...(steps === undefined ? {} : { steps }),
    ...(seed === undefined ? {} : { seed }),
    ...(engine !== 'acestep-cpp' ? { output_format: miniMaxOutputFormat(settings.output_format) } : {}),
    ...(engine === 'minimaxmusic-cpp' && lmSeed !== undefined ? { lm_seed: lmSeed } : {}),
    ...(engine === 'acestep-cpp' && guidance !== undefined ? { ace: { guidance_scale: guidance } } : {}),
    ...((engine === 'audio.cpp' || engine === 'hotstep') && guidance !== undefined ? { cfg_scale: guidance } : {}),
  };
}

export function miniMaxOutputFormat(value: unknown): NonNullable<YueRequest['output_format']> {
  return value === 'mp3' || value === 'wav32' ? value : 'flac';
}

export function useNativeMusicProfiles(engine: ProfileMusicEngine | undefined) {
  const [preferences, setPreferences] = useState(loadPreferences);
  const [legacyAce, setLegacyAce] = useState<{ models: string[]; settings: Settings }>();
  const [refreshCount, setRefreshCount] = useState(0);
  const [response, setResponse] = useState<{ engine: ProfileMusicEngine; data?: NativeProfiles; error?: boolean }>();
  useEffect(() => {
    if (!engine) return;
    const controller = new AbortController();
    setResponse(undefined);
    void (async () => {
      try {
        const url = engine === 'audio.cpp' ? '/v1/music/audio-cpp/models' : `/v1/music/native/${engine}/profiles`;
        const reply = await fetch(apiUrl(url), { signal: controller.signal });
        if (!reply.ok) throw new Error(`HTTP ${reply.status}`);
        const body = await reply.json();
        const data: NativeProfiles = engine === 'audio.cpp' && body.engine === engine && Array.isArray(body.models)
          ? { engine, engine_installed: body.engine_installed !== false, profiles: body.models
              .map((model: { id: string; name?: string; profile_id: string; quantization: string; ready: boolean; missing: string[]; bytes?: number; installable?: boolean; license_url?: string }) => ({
                id: model.id, label: [model.name ?? model.id, model.profile_id, model.quantization].filter(Boolean).join(' · '), total_bytes: model.bytes ?? 0,
                ready: model.ready, missing: model.missing, installable: model.installable, license_url: model.license_url,
              })) }
          : body;
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
  const remembered = engine ? preferences.selected[engine] ?? (engine === 'audio.cpp' ? audioCppMiniMax : engine === 'hotstep' ? 'heartmula-mulacover' : undefined) : undefined;
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
    selectedId: remembered,
    error: result?.error === true,
    loading: Boolean(engine && !result),
    installed: result?.data?.engine_installed === true,
    ready: result?.data?.engine_installed === true && profile?.ready === true,
    originalModels: pending?.models,
    settings: pending?.settings ?? (pair ? preferences.settings[pair] ?? {} : {}),
    refresh: () => { setResponse(undefined); setRefreshCount(count => count + 1); },
    restore: (source: ProfileMusicEngine, id: string, saved: Record<string, unknown>) => {
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
      if (source === 'audio.cpp' || source === 'hotstep') {
        number('guidance_scale', saved.guidance_scale ?? saved.cfg_scale);
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
    select: (id: string, target = engine) => {
      const legacy = engine === target ? pending : undefined;
      if (!target || (legacy && !id)) return;
      if (legacy) setLegacyAce(undefined);
      update({
        selected: { ...preferences.selected, [target]: id },
        settings: legacy
          ? { ...preferences.settings, [JSON.stringify([target, id])]: legacy.settings }
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
