import { useCallback, useEffect, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { rvcApplyStrings } from '../i18n/rvcApply';
import { apiUrl } from '../services/apiBase';
import { responseJson, type Voice } from '../services/rvcVoices';
import { RvcNativeRuntime } from './RvcNativeRuntime';

const URL = '/v1/rvc/apply';
const accompaniment = ['drums', 'bass', 'other', 'guitar', 'piano'];
export interface RvcSourceSong { id: string; title: string; stems: string[] }
type Engine = 'vocal-fast-rs' | 'audio.cpp';
type Backend = 'auto' | Engine;
const BACKEND_STORAGE = 'studio.rvc.backend';
interface Setup {
  effective_backend: Engine | null;
  backends: Record<Engine, { ready: boolean; can_start?: boolean; reason?: string; missing_bytes?: number }>;
}
interface Take { song_id: string; audio_url: string; requested_backend: Backend; effective_backend: Engine; fallback_reason?: string | null }

export function RvcApplyVoice({ song, voice, control, onSaved }: {
  song: RvcSourceSong;
  voice?: Voice;
  control: string;
  onSaved?: () => void;
}) {
  const { language } = useI18n();
  const labels = rvcApplyStrings[language];
  const [setup, setSetup] = useState<Setup>();
  const [setupRequest, setSetupRequest] = useState(0);
  const reloadSetup = useCallback(() => { setSetupRequest(request => request + 1); }, []);
  const [backend, setBackend] = useState<Backend>(() => {
    const stored = localStorage.getItem(BACKEND_STORAGE);
    return stored === 'audio.cpp' || stored === 'vocal-fast-rs' ? stored : 'auto';
  });
  const [shift, setShift] = useState(0);
  const [blend, setBlend] = useState(0);
  const [busy, setBusy] = useState<'install' | 'convert'>();
  const [error, setError] = useState<string>();
  const [take, setTake] = useState<Take>();
  const hasStems = song.stems.includes('vocals')
    && (song.stems.includes('instrumental') || accompaniment.every(stem => song.stems.includes(stem)));

  const fastSetup = setup?.backends['vocal-fast-rs'];
  const fallbackSetup = setup?.backends['audio.cpp'];
  const candidate = backend === 'auto'
    ? fastSetup?.can_start ? 'vocal-fast-rs' : setup?.effective_backend
    : backend;
  const backendStatus = candidate && setup?.backends[candidate];
  const ready = Boolean(backendStatus && (backendStatus.ready || backendStatus.can_start));

  useEffect(() => {
    const controller = new AbortController();
    void fetch(`${URL}/setup`, { signal: controller.signal }).then(responseJson<Setup>)
      .then(status => { if (!controller.signal.aborted) setSetup(status); })
      .catch((failure: Error) => { if (!controller.signal.aborted) setError(failure.message); });
    return () => controller.abort();
  }, [setupRequest]);

  async function install() {
    setBusy('install');
    setError(undefined);
    try {
      const status = await responseJson<Setup>(await fetch(`${URL}/setup`, { method: 'POST' }));
      setSetup(status);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(undefined);
    }
  }

  async function apply() {
    if (!voice || !hasStems || !ready || busy) return;
    setBusy('convert');
    setError(undefined);
    setTake(undefined);
    try {
      const result = await responseJson<Take>(await fetch(URL, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: song.id, voice_id: voice.id, semitone_shift: shift,
          retrieval_blend: voice.index_name ? blend : 0, backend }),
      }));
      setTake(result);
      onSaved?.();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(undefined);
    }
  }

  const mb = fallbackSetup?.missing_bytes
    ? new Intl.NumberFormat(language, { style: 'unit', unit: 'megabyte', maximumFractionDigits: 0 }).format(fallbackSetup.missing_bytes / 1_000_000)
    : '';
  return (
    <div className="mt-3 space-y-2 rounded-md border border-zinc-200 p-3 text-xs dark:border-white/10">
      <p className="font-semibold">{song.title}</p>
      <p className="text-zinc-600 dark:text-zinc-300">{labels.keepOriginal}</p>
      {!hasStems && <p role="status">{labels.stemsNeeded}</p>}
      <label className="block space-y-1">
        <span>{labels.backend}</span>
        <select className={control} value={backend} disabled={!!busy} onChange={event => {
          const next = event.target.value as Backend;
          setBackend(next);
          localStorage.setItem(BACKEND_STORAGE, next);
        }}>
          <option value="auto">{labels.auto}</option>
          <option value="vocal-fast-rs">vocal-fast-rs</option>
          <option value="audio.cpp">audio.cpp</option>
        </select>
      </label>
      {!setup && !error && <p role="status">{labels.checking}</p>}
      {candidate && backendStatus && backendStatus.ready && <p>{labels.effective.replace('{backend}', candidate)}</p>}
      {candidate && backendStatus && !backendStatus.ready && backendStatus.can_start && (
        <p role="status">{labels.onStart.replace('{backend}', candidate)}</p>
      )}
      {backend === 'vocal-fast-rs' && !ready && fastSetup?.reason && <p role="status">{fastSetup.reason}</p>}
      <div className="grid gap-2 sm:grid-cols-2">
        <label className="block space-y-1">
          <span>{labels.pitch}</span>
          <input className={control} type="number" min={-24} max={24} step={1} value={shift}
            disabled={!!busy} onChange={event => setShift(Math.min(24, Math.max(-24, Math.trunc(Number(event.target.value)))))}/>
        </label>
        <label className="block space-y-1">
          <span>{labels.blend}</span>
          <input className="block w-full" type="range" min={0} max={1} step={0.05} value={voice?.index_name ? blend : 0}
            disabled={!!busy || !voice?.index_name} onChange={event => setBlend(Number(event.target.value))}/>
          {voice && !voice.index_name && <span className="text-zinc-500">{labels.noIndex}</span>}
        </label>
      </div>
      {backend !== 'audio.cpp' && <RvcNativeRuntime onInstalled={reloadSetup} disabled={!!busy} />}
      {backend !== 'vocal-fast-rs' && fallbackSetup && !fallbackSetup.ready && !ready && (
        <button type="button" disabled={!!busy} className="px-2 py-1 font-semibold text-pink-600 underline disabled:opacity-50 dark:text-pink-300"
          onClick={() => void install()}>{busy === 'install' ? labels.installing : `${labels.install}${mb ? ` · ${mb}` : ''}`}</button>
      )}
      {!setup && error && <button type="button" className="text-pink-600 underline dark:text-pink-300"
        onClick={() => { setError(undefined); reloadSetup(); }}>{labels.retry}</button>}
      <button type="button" disabled={!voice || !hasStems || !ready || !!busy}
        className="rounded border border-zinc-200 px-3 py-2 font-semibold disabled:opacity-50 dark:border-white/10"
        onClick={() => void apply()}>{labels.apply}</button>
      {busy === 'convert' && <p role="status">{labels.converting}</p>}
      {take && (
        <div className="space-y-1">
          <p role="status">{labels.saved.replace('{backend}', take.effective_backend)}</p>
          {take.requested_backend !== take.effective_backend && (
            <p>{labels.requested.replace('{backend}', take.requested_backend === 'auto' ? labels.auto : take.requested_backend)}</p>
          )}
          {take.fallback_reason && <p>{labels.fallbackReason} {take.fallback_reason}</p>}
          <audio className="w-full" controls src={apiUrl(take.audio_url)} />
        </div>
      )}
      {error && <p role="alert" className="text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
}
