import { useSyncExternalStore } from 'react';
import { responseJson } from './rvcVoices';

export const RVC_VOICE_STORAGE = 'studio.rvc.voiceId';
export const RVC_BACKEND_STORAGE = 'studio.rvc.backend';
export const RVC_QUICK_EVENT = 'yue:rvc-quick';
const SELECTION_CHANGED = 'yue:rvc-selection';

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
  lastUpdate = lastUpdate.then(async () => {
    await responseJson(await fetch('/v1/rvc/selection', {
      method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
    }));
  }).catch(() => { /* The local action still carries the chosen voice and backend. */ });
}

export function clearLastRvcVoice(voiceId: string) {
  if (readLastVoice() !== voiceId) return;
  localStorage.removeItem(RVC_VOICE_STORAGE);
  window.dispatchEvent(new Event(SELECTION_CHANGED));
}
