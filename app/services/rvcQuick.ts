import { useSyncExternalStore } from 'react';
import { responseJson } from './rvcVoices';

export const RVC_VOICE_STORAGE = 'studio.rvc.voiceId';
export const RVC_BACKEND_STORAGE = 'studio.rvc.backend';
export const RVC_QUICK_EVENT = 'yue:rvc-quick';
const SELECTION_CHANGED = 'yue:rvc-selection';
const SELECTION_URL = '/v1/rvc/selection';

export type RvcBackend = 'auto' | 'vocal-fast-rs' | 'audio.cpp';
export interface RvcSelection { voice_id: string; backend: RvcBackend }

export function readRvcBackend(): RvcBackend {
  const value = localStorage.getItem(RVC_BACKEND_STORAGE);
  return value === 'audio.cpp' || value === 'vocal-fast-rs' ? value : 'auto';
}

function readLastVoice() {
  return localStorage.getItem(RVC_VOICE_STORAGE) ?? '';
}

function listenSelection(onChange: () => void) {
  window.addEventListener(SELECTION_CHANGED, onChange);
  window.addEventListener('storage', onChange);
  return () => {
    window.removeEventListener(SELECTION_CHANGED, onChange);
    window.removeEventListener('storage', onChange);
  };
}

export function useLastRvcVoice() {
  return useSyncExternalStore(listenSelection, readLastVoice);
}

let lastUpdate = Promise.resolve();
export function chooseLastRvcVoice(voiceId: string, backend = readRvcBackend()) {
  localStorage.setItem(RVC_VOICE_STORAGE, voiceId);
  window.dispatchEvent(new Event(SELECTION_CHANGED));
  const body: RvcSelection = { voice_id: voiceId, backend };
  const save = globalThis.fetch;
  lastUpdate = lastUpdate.then(async () => {
    await responseJson(await save(SELECTION_URL, {
      method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
    }));
  }).catch(() => { /* The local action still carries the chosen voice and backend. */ });
}

export function clearLastRvcVoice(voiceId: string) {
  if (readLastVoice() !== voiceId) return;
  localStorage.removeItem(RVC_VOICE_STORAGE);
  window.dispatchEvent(new Event(SELECTION_CHANGED));
}

export async function restoreLastRvcVoice(signal: AbortSignal) {
  const before = readLastVoice();
  try {
    const selection = await responseJson<RvcSelection | null>(await fetch(SELECTION_URL, { signal }));
    if (signal.aborted || readLastVoice() !== before) return;
    if (selection?.voice_id) {
      localStorage.setItem(RVC_VOICE_STORAGE, selection.voice_id);
      localStorage.setItem(RVC_BACKEND_STORAGE, selection.backend);
      window.dispatchEvent(new Event(SELECTION_CHANGED));
    } else if (before) {
      chooseLastRvcVoice(before);
    }
  } catch { /* Keep the locally remembered voice if the server is unavailable. */ }
}
