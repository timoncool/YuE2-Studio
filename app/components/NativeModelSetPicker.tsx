import React, { useEffect, useRef, useState } from 'react';
import { CheckCircle2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { nativeModelSetStrings } from '../i18n/nativeModelSet';
import { apiUrl } from '../services/apiBase';
import type { NativeProfile, SetDownload } from '../services/nativeMusicProfiles';
import { WorkProgress } from './WorkProgress';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';

type Engine = 'acestep-cpp' | 'minimaxmusic-cpp' | 'yue2-cpp';
/** YuE2's own setup download: one job, the set's files by component id. */
type SetupJob = { status: 'downloading' | 'completed' | 'cancelled' | 'failed'; downloaded_bytes: number; total_bytes: number; error?: string | null };
type Runtime = { supported: boolean; installed?: boolean; bytes?: number; download?: SetDownload | null };
type Phase = 'idle' | 'engine' | 'model';

async function answer(response: Response): Promise<Record<string, unknown>> {
  const body = (await response.json().catch(() => null)) as Record<string, unknown> | null;
  if (!response.ok) throw new Error(typeof body?.error === 'string' ? body.error : `HTTP ${response.status}`);
  return body ?? {};
}

/**
 * The quality of a native model and the one button that brings it: the engine first when it is missing, then the
 * set's weights, with one bar for both. The main sets are offered with the one this card holds best marked; the
 * variants for particular cards wait under "all variants".
 */
export function NativeModelSetPicker({ engine, name, profiles, selected, engineInstalled, onSelect, onChanged, onReady }: {
  engine: Engine;
  name: string;
  profiles: NativeProfile[];
  selected?: NativeProfile;
  engineInstalled: boolean;
  onSelect: (id: string) => void;
  onChanged: () => void;
  /** The chosen set has every file now: the pair it belongs to can be asked for again. */
  onReady?: () => void;
}) {
  const { language } = useI18n();
  const text = nativeModelSetStrings[language];
  const [showAll, setShowAll] = useState(() => Boolean(selected?.advanced));
  const [phase, setPhase] = useState<Phase>('idle');
  const [runtime, setRuntime] = useState<Runtime>();
  const [download, setDownload] = useState<SetDownload | null>();
  const [error, setError] = useState('');
  const changed = useRef(onChanged);
  useEffect(() => { changed.current = onChanged; }, [onChanged]);
  const readied = useRef(onReady);
  useEffect(() => { readied.current = onReady; }, [onReady]);
  // YuE2 ships its engine with the studio and downloads its sets through the studio's setup
  const setup = engine === 'yue2-cpp';
  const runtimeUrl = apiUrl(`/v1/music/native/${engine}/runtime`);
  const installUrl = apiUrl(`/v1/music/native/${engine}/profiles/install`);
  const gb = (bytes: number) => new Intl.NumberFormat(language, { maximumFractionDigits: 1, minimumFractionDigits: 1 }).format(bytes / 2 ** 30);
  const label = (profile: NativeProfile) => profile.labels?.[language] ?? profile.labels?.en ?? profile.label;

  useEffect(() => {
    if (selected?.installing) setPhase('model');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected?.id]);

  useEffect(() => {
    if (engineInstalled || setup) return;
    const abort = new AbortController();
    void fetch(runtimeUrl, { signal: abort.signal }).then(answer).then(body => setRuntime(body as Runtime)).catch(() => undefined);
    return () => abort.abort();
  }, [engineInstalled, runtimeUrl]);

  useEffect(() => {
    if (phase === 'idle') return;
    const abort = new AbortController();
    const poll = async () => {
      try {
        if (phase === 'engine') {
          const state = await answer(await fetch(runtimeUrl, { signal: abort.signal })) as Runtime;
          setRuntime(state);
          setDownload(state.download);
          if (state.installed) {
            setPhase('model');
            setDownload(undefined);
            changed.current();
            await startWeights();
          } else if (state.download?.done && state.download.error) {
            setError(state.download.error);
            setPhase('idle');
          }
          return;
        }
        if (setup) {
          const status = await answer(await fetch(apiUrl('/setup/status'), { signal: abort.signal }));
          const job = status.active as SetupJob | null | undefined;
          const installed = new Set((status.installed_components as string[] | undefined) ?? []);
          if (job) setDownload({ downloaded_bytes: job.downloaded_bytes, total_bytes: job.total_bytes, done: job.status !== 'downloading', error: job.error });
          if (selected?.components?.every(id => installed.has(id))) {
            setPhase('idle');
            changed.current();
            readied.current?.();
          } else if (job?.status === 'failed' || job?.status === 'cancelled') {
            if (job.status === 'failed') setError(job.error ?? text.failed);
            setPhase('idle');
          }
          return;
        }
        const catalog = await answer(await fetch(apiUrl(`/v1/music/native/${engine}/profiles`), { signal: abort.signal }));
        const row = (catalog.profiles as NativeProfile[] | undefined)?.find(profile => profile.id === selected?.id);
        setDownload(row?.download);
        if (row?.ready) {
          setPhase('idle');
          changed.current();
          readied.current?.();
        } else if (row?.download?.done && row.download.error) {
          setError(row.download.error);
          setPhase('idle');
        }
      } catch (problem) {
        if (!abort.signal.aborted) setError(problem instanceof Error ? problem.message : String(problem));
      }
    };
    const timer = window.setInterval(() => void poll(), 1000);
    void poll();
    return () => { abort.abort(); window.clearInterval(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase, engine, runtimeUrl, selected?.id]);

  async function startWeights() {
    if (!selected) return;
    if (setup) {
      await answer(await fetch(apiUrl('/setup/download'), {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ profile_id: selected.id }),
      }));
      return;
    }
    const body = await answer(await fetch(installUrl, {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ model: selected.id }),
    }));
    if (body.ready === true) {
      setPhase('idle');
      changed.current();
      readied.current?.();
    }
  }

  async function begin() {
    setError('');
    setDownload(undefined);
    try {
      if (!engineInstalled) {
        setPhase('engine');
        const body = await answer(await fetch(runtimeUrl, { method: 'POST' }));
        if (body.started !== true && body.installed !== true) throw new Error(typeof body.error === 'string' ? body.error : text.failed);
        return;
      }
      setPhase('model');
      await startWeights();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
      setPhase('idle');
    }
  }

  function cancel() {
    const request = setup
      ? fetch(apiUrl('/setup/cancel'), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })
      : phase === 'engine'
      ? fetch(runtimeUrl, { method: 'DELETE' })
      : fetch(installUrl, { method: 'DELETE', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ model: selected?.id }) });
    void request.catch(problem => setError(String(problem)));
    setPhase('idle');
  }

  const main = profiles.filter(profile => !profile.advanced);
  const advanced = profiles.filter(profile => profile.advanced);
  const shown = showAll ? profiles : main;
  const ready = engineInstalled && selected?.ready === true;
  const engineBytes = engineInstalled ? 0 : runtime?.bytes ?? 0;
  const total = engineBytes + (selected?.ready ? 0 : selected?.missing_bytes ?? selected?.total_bytes ?? 0);
  const percent = download?.total_bytes ? Math.min(100, Math.floor(100 * download.downloaded_bytes / download.total_bytes)) : null;
  const busy = phase !== 'idle';

  return (
    <fieldset className="space-y-2">
      <legend className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{text.quality}</legend>
      <div role="radiogroup" aria-label={text.quality} className="space-y-1">
        {shown.map(profile => (
          <label key={profile.id} className={`flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-sm transition-colors ${
            profile.id === selected?.id ? 'border-pink-500 bg-pink-500/5' : 'border-zinc-200 hover:border-pink-300 dark:border-white/10'}`}>
            <input type="radio" name={`${engine}-set`} value={profile.id} className="accent-pink-600" checked={profile.id === selected?.id}
              disabled={busy} onChange={() => onSelect(profile.id)} />
            <span className="min-w-0 flex-1">
              <span className="font-medium">{label(profile)}</span>
              {profile.vram_gb ? <span className="ml-1.5 text-xs text-zinc-500 dark:text-zinc-400">{text.fromVram(new Intl.NumberFormat(language, { maximumFractionDigits: 1 }).format(profile.vram_gb))}</span> : null}
            </span>
            {profile.recommended && <span className="shrink-0 rounded-full bg-emerald-500/10 px-2 py-0.5 text-[10px] font-semibold text-emerald-600 dark:text-emerald-300">{text.recommended}</span>}
            {profile.ready && engineInstalled && <CheckCircle2 size={14} className="shrink-0 text-emerald-500" aria-hidden />}
          </label>
        ))}
      </div>
      {advanced.length > 0 && !showAll && (
        <button type="button" className="min-h-6 text-xs font-medium text-zinc-500 hover:text-pink-600 dark:text-zinc-400" onClick={() => setShowAll(true)}>
          {text.allVariants}
        </button>
      )}
      {selected && (ready ? (
        <p role="status" className="flex items-center gap-1.5 text-xs font-medium text-emerald-600 dark:text-emerald-300">
          <CheckCircle2 size={14} aria-hidden />{text.ready}
        </p>
      ) : busy ? (
        <div>
          <WorkProgress label={`${phase === 'engine' ? text.engineStage : text.modelStage}${download?.total_bytes ? ` · ${text.of(gb(download.downloaded_bytes), gb(download.total_bytes))}` : ''}`} percent={percent} />
          <SecondaryButton className="mt-2" onClick={cancel}>{text.cancel}</SecondaryButton>
        </div>
      ) : (
        <PrimaryButton className="w-full" onClick={() => void begin()}>
          {text.download(name, gb(total))}
        </PrimaryButton>
      ))}
      {error && phase === 'idle' && <p role="alert" className="text-xs text-red-600 dark:text-red-400">{text.failed} {error}</p>}
    </fieldset>
  );
}
