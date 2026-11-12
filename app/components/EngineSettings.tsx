import React, { useCallback, useEffect, useState } from 'react';
import { SELF_READ } from '../services/studioQueries';
import { AlertTriangle, Check, Cpu, FolderOpen, Loader2, RotateCw , Terminal } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { poll } from '../services/poll';

/**
 * Launch options for the local engine.
 *
 * These are `yue-server` command-line flags, read once at startup, so saving
 * them restarts the engine. `--max-batch` is the ceiling on how many songs one
 * request may compose, which is why the create panel clamps to it.
 */

type ComputeBackend = 'auto' | 'cuda' | 'vulkan' | 'cpu';
type FlashAttention = 'auto' | 'on' | 'off';
type KvQ8Off = 'processor' | 'vulkan' | 'flash_attention_off' | null;

/** What the running engine was started with, as the service read it from the launch. */
interface InEffect {
  max_seq: number;
  /** `fitted_before`: fitted when this engine started and reset since; the next start takes the saved value. */
  max_seq_reason: 'full' | 'set' | 'fitted' | 'fitted_before';
  flash_attention: boolean;
  nar_chunk_seconds: number | null;
  /** Fewer tracks a request than Settings asks: that many fit the card's free memory when the engine started. */
  max_batch_for_card?: number | null;
}

interface EngineOptions {
  backend: ComputeBackend;
  keep_loaded: boolean;
  max_batch: number | null;
  max_seq: number | null;
  vae_core: number | null;
  vae_halo: number | null;
  /** Seconds a later chunk of the sound hears the song again before its fade; null: 30. */
  chunk_overlap_seconds: number | null;
  flash_attention: FlashAttention;
  kv_q8: boolean;
  clamp_fp16: boolean;
  /** The NVIDIA card by nvidia-smi index; null: the first. Taken at the studio's start. */
  gpu?: number | null;
  /** Any other launch flag, as one line. It reaches the engine after the flags above. */
  extra_args: string;
  /** A hand-built engine (yue2.cpp) to run instead of the bundled one; null: the bundled engine. */
  engine_bin?: string | null;
}

const DEFAULTS: EngineOptions = {
  backend: 'auto',
  keep_loaded: false,
  max_batch: null,
  max_seq: null,
  vae_core: null,
  vae_halo: null,
  chunk_overlap_seconds: null,
  flash_attention: 'auto',
  kv_q8: false,
  clamp_fp16: false,
  gpu: null,
  extra_args: '',
  engine_bin: null,
};

type NumberKey = 'max_batch' | 'max_seq' | 'vae_core' | 'vae_halo' | 'chunk_overlap_seconds';

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

const Toggle: React.FC<{ label: string; hint: string; checked: boolean; onChange: (value: boolean) => void; disabled?: boolean; note?: string | null }> = ({ label, hint, checked, onChange, disabled, note }) => (
  <label className={`flex items-start gap-3 rounded-xl border border-zinc-200 p-3 dark:border-white/10 ${disabled ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'}`}>
    <input type="checkbox" checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} className="mt-0.5 h-4 w-4 accent-pink-500" />
    <span className="min-w-0">
      <span className="block text-sm font-medium text-zinc-800 dark:text-zinc-100">{label}</span>
      <span className="mt-0.5 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{hint}</span>
      {note && <span className="mt-0.5 block text-xs leading-5 text-amber-600 dark:text-amber-300">{note}</span>}
    </span>
  </label>
);

export const EngineSettings: React.FC = () => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [options, setOptions] = useState<EngineOptions>(DEFAULTS);
  const [saved, setSaved] = useState<EngineOptions>(DEFAULTS);
  // Which engine executable a start takes and which one is running, so the
  // page answers "did my change apply?" without opening the engine log.
  const [runtime, setRuntime] = useState<{ executable: string | null; executable_source: string; running_executable: string | null; engine_starting: boolean } | null>(null);
  // The KV cache and flash attention of the running engine, and the cache the saved settings start the next one with.
  const [inEffect, setInEffect] = useState<InEffect | null>(null);
  // why a KV cache in Q8 cannot apply here, as the service reads the machine (an Auto it resolves to the processor)
  const [serverKvQ8Off, setServerKvQ8Off] = useState<KvQ8Off>(null);
  const kvQ8Off: KvQ8Off = options.backend === 'cpu' ? 'processor' : options.backend === 'vulkan' ? 'vulkan' : options.flash_attention === 'off' ? 'flash_attention_off'
    : options.backend === saved.backend && options.flash_attention === saved.flash_attention ? serverKvQ8Off : null;
  const [savedMaxSeq, setSavedMaxSeq] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [picking, setPicking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The native file picker belongs to the studio process: this window is a
  // browser and cannot open one. Nothing is changed until the user saves,
  // the same as every other field on this page.
  const browseEngine = async () => {
    setPicking(true);
    setError(null);
    try {
      const response = await fetch('/engine/pick-executable', { method: 'POST' });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Opening the file picker failed (${response.status})`);
      if (body?.picked && typeof body.path === 'string') {
        setOptions(current => ({ ...current, engine_bin: body.path }));
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Opening the file picker failed.');
    } finally {
      setPicking(false);
    }
  };

  const load = useCallback(async () => {
    try {
      const response = await fetch('/engine/options');
      if (!response.ok) throw new Error(`Engine options are unavailable (${response.status})`);
      const body: { options: EngineOptions; in_effect?: InEffect | null; saved_max_seq?: number; kv_q8_off_reason?: KvQ8Off; executable?: string | null; executable_source?: string; running_executable?: string | null; engine_starting?: boolean } = await response.json();
      setInEffect(body.in_effect ?? null);
      setServerKvQ8Off(body.kv_q8_off_reason ?? null);
      setSavedMaxSeq(typeof body.saved_max_seq === 'number' ? body.saved_max_seq : null);
      // A settings file written before a flag existed arrives without it;
      // the default keeps the comparison below honest.
      setOptions({ ...DEFAULTS, ...body.options });
      setSaved({ ...DEFAULTS, ...body.options });
      setRuntime({
        executable: body.executable ?? null,
        executable_source: body.executable_source ?? 'bundled',
        running_executable: body.running_executable ?? null,
        engine_starting: body.engine_starting === true,
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not read the engine options.');
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const dirty = JSON.stringify(options) !== JSON.stringify(saved);

  // The cache stepped down after the card ran out of memory goes back to the model's full context.
  const resetFitted = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const response = await fetch('/engine/options/fitted', { method: 'DELETE' });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Resetting the fitted KV cache failed (${response.status})`);
      setNotice(body?.engine_restarted ? t('engineOptionsSavedRestarted') : t('engineOptionsSavedLater'));
      void load();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Resetting the fitted KV cache failed.');
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const response = await fetch('/engine/options', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(options),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Saving the engine options failed (${response.status})`);
      setSaved(body.options);
      setNotice(body.engine_restarted
        ? t('engineOptionsSavedRestarted')
        : t('engineOptionsSavedLater'));
      // The restart, or the lack of one, changed which engine is running.
      void load();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Saving the engine options failed.');
    } finally {
      setBusy(false);
    }
  };

  // What the engine is saying. It used to be on a page of its own; it belongs
  // with the flags that started the process.
  const [logs, setLogs] = useState<string[]>([]);
  const [logsOpen, setLogsOpen] = useState(false);
  useEffect(() => {
    if (!logsOpen) return;
    const read = () => fetch('/v1/engine/logs', { headers: SELF_READ })
      .then(response => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((body: { lines?: string[] }) => setLogs((body.lines ?? []).slice(-200)))
      .catch(() => undefined);
    return poll(read, 2000);
  }, [logsOpen]);

  const restart = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const response = await fetch('/engine/restart', { method: 'POST' });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Restarting the engine failed (${response.status})`);
      setNotice(t('engineRestarted'));
      void load();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Restarting the engine failed.');
    } finally {
      setBusy(false);
    }
  };

  // With two cards or more, which one the studio computes on
  const [cards, setCards] = useState<{ cards: { index: number; name: string; memory_gb: number }[] | null; running: number | null } | null>(null);
  useEffect(() => {
    void fetch('/v1/system/gpus', { headers: SELF_READ }).then(response => (response.ok ? response.json() : null)).then(setCards).catch(() => undefined);
  }, []);
  const runningCard = cards?.running ?? null;
  const chosenCard = options.gpu ?? null;

  const numberField = (key: NumberKey, label: string, placeholder: string, hint: string, min: number, max: number, step: number) => (
    <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
      <span className="mb-1.5 block">{label}</span>
      <input
        type="number"
        min={min}
        max={max}
        step={step}
        placeholder={placeholder}
        value={options[key] ?? ''}
        onChange={event => {
          const value = Number(event.target.value);
          setOptions(current => ({ ...current, [key]: event.target.value === '' || !Number.isFinite(value) ? null : value }));
        }}
        className={CONTROL}
      />
      <span className="mt-1 block font-normal text-zinc-500">{hint}</span>
    </label>
  );

  return (
    <div className="space-y-3">
      <h4 className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
        <Cpu size={16} className="text-pink-500" /> {t('localEngine')}
      </h4>

      <div className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <span className="block text-sm font-medium text-zinc-800 dark:text-zinc-100">{tt('computeBackendLabel')}</span>
        <div className="mt-2 grid grid-cols-2 gap-1 rounded-lg bg-zinc-100 p-1 sm:grid-cols-4 dark:bg-black/30" role="radiogroup" aria-label={tt('computeBackendLabel')}>
          {(['auto', 'cuda', 'vulkan', 'cpu'] as const).map(value => (
            <button
              key={value}
              type="button"
              role="radio"
              aria-checked={options.backend === value}
              onClick={() => setOptions(current => ({ ...current, backend: value }))}
              className={`rounded-md px-2 py-1.5 text-xs font-semibold transition-all ${options.backend === value ? 'bg-white text-black shadow-xs dark:bg-zinc-700 dark:text-white' : 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200'}`}
            >
              {tt(`computeBackend_${value}`)}
            </button>
          ))}
        </div>
        <span className="mt-2 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{tt(`computeBackendHint_${options.backend}`)}</span>
      </div>

      {cards?.cards && (cards.cards.length > 1 || (chosenCard !== null && !cards.cards.some(card => card.index === chosenCard))) && (
        <label className="block rounded-xl border border-zinc-200 p-3 dark:border-white/10">
          <span className="block text-sm font-medium text-zinc-800 dark:text-zinc-100">{t('gpuChoice')}</span>
          <select
            value={chosenCard === null ? '' : String(chosenCard)}
            onChange={event => setOptions(current => ({ ...current, gpu: event.target.value === '' ? null : Number(event.target.value) }))}
            className={`${CONTROL} mt-2`}
          >
            <option value="">{t('gpuChoiceFirst')}</option>
            {chosenCard !== null && !cards.cards.some(card => card.index === chosenCard) && <option value={String(chosenCard)}>{t('gpuChoiceMissing').replace('{index}', String(chosenCard))}</option>}
            {cards.cards.map(card => <option key={card.index} value={card.index}>{`${card.index}: ${card.name} · ${card.memory_gb.toFixed(0)} GB`}</option>)}
          </select>
          {chosenCard !== null && !cards.cards.some(card => card.index === chosenCard) ? (
            <span className="mt-2 block text-xs leading-5 text-amber-600 dark:text-amber-300">{t('gpuChoiceGone').replace('{index}', String(chosenCard))}</span>
          ) : (
            <span className="mt-2 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">
              {chosenCard !== runningCard ? t('gpuChoiceRestart') : t('gpuChoiceHint')}
            </span>
          )}
        </label>
      )}

      <div className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <span className="block text-sm font-medium text-zinc-800 dark:text-zinc-100">{t('engineBinLabel')}</span>
        <input
          type="text"
          value={options.engine_bin ?? ''}
          onChange={event => setOptions(current => ({ ...current, engine_bin: event.target.value === '' ? null : event.target.value }))}
          spellCheck={false}
          placeholder={t('engineBinBundled')}
          className={`${CONTROL} mt-2 font-mono text-xs`}
        />
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => void browseEngine()}
            disabled={busy || picking}
            className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
          >
            {picking ? <Loader2 size={13} className="animate-spin" /> : <FolderOpen size={13} />} {t('engineBinBrowse')}
          </button>
          <button
            type="button"
            onClick={() => setOptions(current => ({ ...current, engine_bin: null }))}
            disabled={!options.engine_bin || busy}
            className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
          >
            {t('engineBinBundled')}
          </button>
        </div>
        <span className="mt-2 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('engineBinHint')}</span>
        {runtime && (
          <span className="mt-1 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">
            {runtime.running_executable ? `${t('engineBinRunning')}: ${runtime.running_executable}` : runtime.engine_starting ? t('engineBinStarting') : t('engineBinNotRunning')}
            {runtime.executable && runtime.executable !== runtime.running_executable ? ` · ${t('engineBinNextStart')}: ${runtime.executable}` : ''}
            {runtime.executable_source === 'environment' ? ` · ${t('engineBinEnvOverride')}` : ''}
          </span>
        )}
      </div>

      <Toggle
        label={t('keepLoadedLabel')}
        hint={t('keepLoadedHint')}
        checked={options.keep_loaded}
        onChange={value => setOptions(current => ({ ...current, keep_loaded: value }))}
      />

      <div className="grid gap-3 sm:grid-cols-2">
        {numberField('max_batch', t('maxBatchLabel'), '1', t('maxBatchHint'), 1, 8, 1)}
        {numberField('max_seq', t('maxSeqLabel'), tt('maxSeqAuto'), t('maxSeqHintYue'), 8192, 24576, 1024)}
        {numberField('chunk_overlap_seconds', t('chunkOverlapLabel'), '30', t('chunkOverlapHint'), 0, 45, 1)}
      </div>
      {inEffect && (
        <div className="-mt-1 space-y-0.5 text-xs leading-5 text-zinc-500 dark:text-zinc-400">
          <span className="flex flex-wrap items-center gap-2">
            {tt('maxSeqNow').replace('{n}', String(inEffect.max_seq)).replace('{reason}', tt(
              inEffect.max_seq_reason === 'set' ? 'maxSeqReasonSet' : inEffect.max_seq_reason === 'full' ? 'maxSeqReasonFull' : 'maxSeqReasonFitted',
            ))}
            {inEffect.max_seq_reason === 'fitted' && (
              <button
                type="button"
                onClick={() => void resetFitted()}
                disabled={busy}
                title={tt('fittedResetHint')}
                className="rounded-md border border-zinc-300 px-2 py-0.5 text-[11px] font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
              >
                {tt('fittedReset')}
              </button>
            )}
          </span>
          {!inEffect.flash_attention && inEffect.nar_chunk_seconds != null && (
            <span className="block">{tt('narChunkNow').replace('{s}', String(inEffect.nar_chunk_seconds))}</span>
          )}
          {inEffect.max_batch_for_card != null && (
            <span className="block">{tt('maxBatchForCard').replace('{n}', String(inEffect.max_batch_for_card))}</span>
          )}
          {savedMaxSeq != null && savedMaxSeq !== inEffect.max_seq && (
            <span className="block text-amber-600 dark:text-amber-300">{tt('maxSeqPending').replace('{n}', String(savedMaxSeq))}</span>
          )}
        </div>
      )}

      <label className="block rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <span className="block text-sm font-medium text-zinc-800 dark:text-zinc-100">{tt('flashAttentionLabel')}</span>
        <select
          value={options.flash_attention}
          onChange={event => setOptions(current => ({ ...current, flash_attention: event.target.value as FlashAttention }))}
          className={`${CONTROL} mt-2`}
        >
          <option value="auto">{tt('flashAttentionAuto')}</option>
          <option value="on">{tt('flashAttentionOn')}</option>
          <option value="off">{tt('flashAttentionOff')}</option>
        </select>
        <span className="mt-2 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{tt('flashAttentionHint')}</span>
      </label>

      <Toggle
        label={tt('kvQ8Label')}
        hint={tt('kvQ8Hint')}
        checked={options.kv_q8}
        disabled={kvQ8Off !== null}
        note={kvQ8Off === 'processor' ? tt('kvQ8OffProcessor') : kvQ8Off === 'vulkan' ? tt('kvQ8OffVulkan') : kvQ8Off === 'flash_attention_off' ? tt('kvQ8OffFlashAttention') : null}
        onChange={value => setOptions(current => ({ ...current, kv_q8: value }))}
      />

      <details className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-zinc-500">{t('diagnostics')}</summary>
        <div className="mt-3 space-y-3">
          <div className="grid gap-3 sm:grid-cols-2">
            {numberField('vae_core', t('vaeCoreLabel'), '512', t('vaeCoreHint'), 64, 4096, 64)}
            {numberField('vae_halo', t('vaeHaloLabel'), '16', t('vaeHaloHint'), 0, 256, 4)}
          </div>
          <Toggle
            label={t('clampFp16Label')}
            hint={t('clampFp16Hint')}
            checked={options.clamp_fp16}
            onChange={value => setOptions(current => ({ ...current, clamp_fp16: value }))}
          />
        </div>
      </details>

      <details className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-zinc-500">{t('extraArgsLabel')}</summary>
        <div className="mt-3 space-y-2">
          <textarea
            value={options.extra_args}
            onChange={event => setOptions(current => ({ ...current, extra_args: event.target.value }))}
            rows={2}
            spellCheck={false}
            placeholder="--flag value --other=1"
            className={`${CONTROL} resize-y font-mono text-xs`}
          />
          <span className="block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('extraArgsHint')}</span>
        </div>
      </details>

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => void save()}
          disabled={!dirty || busy}
          className="inline-flex items-center gap-2 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white disabled:opacity-50"
        >
          {busy ? <Loader2 size={14} className="animate-spin" /> : null} {t('saveAndRestartEngine')}
        </button>
        <button
          type="button"
          // A restart applies what is saved, so anything typed and not saved
          // is saved first: the button does what the page shows.
          onClick={() => void (dirty ? save() : restart())}
          disabled={busy}
          className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
        >
          <RotateCw size={13} /> {t('restartEngine')}
        </button>
      </div>

      <div className="rounded-xl border border-zinc-200 dark:border-white/10">
        <button
          type="button"
          onClick={() => setLogsOpen(open => !open)}
          className="flex w-full items-center gap-2 px-3 py-2 text-[11px] font-bold uppercase tracking-wide text-zinc-500 hover:text-pink-500"
        >
          <Terminal size={13} /> {t('engineLog')}
        </button>
        {logsOpen && (
          <div className="max-h-64 overflow-y-auto border-t border-zinc-200 bg-zinc-50 p-3 font-mono text-[11px] leading-4 text-zinc-600 dark:border-white/10 dark:bg-black/30 dark:text-zinc-400">
            {logs.length === 0 ? <span className="text-zinc-400">—</span> : logs.map((line, index) => <div key={index} className="wrap-break-word">{line}</div>)}
          </div>
        )}
      </div>

      {notice && (
        <p className="flex items-center gap-2 rounded-lg bg-emerald-500/10 px-3 py-2 text-xs text-emerald-700 dark:text-emerald-300">
          <Check size={14} /> {notice}
        </p>
      )}
      {error && (
        <p role="alert" className="flex items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
          <AlertTriangle size={14} /> {error}
        </p>
      )}
    </div>
  );
};
