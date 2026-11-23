import React, { useState } from 'react';
import { ChevronDown, ListChecks, Loader2, Sparkles, Undo2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { readAudioCodes, SCHEDULERS, SOLVERS } from '../services/aceEngine';
import { NumberField } from './NumberField';
import { SecondaryButton } from './kit/Buttons';

/**
 * ACE-Step Studio 4.0.1's own form for the native engine, carried over as it was: the song's metadata, the planner
 * and its strength, the guidance presets, and under Advanced the language-model stage, the DiT stage, the LoRA group
 * scales and the fades. Duration, seed, CFG, steps and the output format are the Create form's own fields.
 */

export type AceStudioDraft = Record<string, string | boolean>;

const KEY = 'studio.aceStudioDraft';
const LANGUAGES = ['', 'en', 'ru', 'zh', 'ja', 'ko', 'es', 'fr', 'de', 'it', 'pt', 'ar', 'hi', 'tr', 'pl', 'uk', 'nl', 'sv', 'vi', 'th', 'id', 'lt', 'unknown'];
const KEYS = ['', ...['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'].flatMap(note => [`${note} major`, `${note} minor`])];
const TIME_SIGNATURES = ['', '2', '3', '4', '6'];
const GUIDANCE = ['apg', 'adg', 'cfg_pp', 'dynamic_cfg', 'rescaled_cfg', 'cfg_zero_star', 'smc_cfg', 'cfg_mp'];
const REP_MODES = ['presence', 'frequency', 'dry'];
const GROUPS = ['self_attn', 'cross_attn', 'mlp', 'cond_embed', 'time_embed', 'proj_in'] as const;
const PRESETS: Array<{ id: string; start: number; end: number; mode: string }> = [
  { id: 'default', start: 0, end: 1, mode: '' },
  { id: 'clean', start: 0, end: 0.5, mode: '' },
  { id: 'creative', start: 0.2, end: 0.8, mode: '' },
  { id: 'cover', start: 0, end: 0.95, mode: '' },
  { id: 'strict', start: 0, end: 0.75, mode: '' },
  { id: 'adg', start: 0, end: 1, mode: 'adg' },
];

export type DitKind = 'turbo' | 'sft' | 'base' | 'merge';
/** What a DiT file is, which decides its defaults; the studio's sets are XL Turbo. */
export const ditKind = (file: string | undefined): DitKind => {
  const name = (file ?? 'turbo').toLowerCase();
  if (name.includes('merge')) return 'merge';
  if (name.includes('turbo') && !name.includes('sftturbo') && !name.includes('base-turbo')) return 'turbo';
  if (name.includes('base')) return 'base';
  if (name.includes('turbo')) return 'turbo';
  return 'sft';
};
export const DIT_DEFAULTS: Record<DitKind, { steps: number; guidance: number; shift: number; solver: string; scheduler: string }> = {
  turbo: { steps: 8, guidance: 1, shift: 3, solver: 'heun', scheduler: 'linear_quadratic' },
  sft: { steps: 50, guidance: 7, shift: 1, solver: 'euler', scheduler: 'linear' },
  base: { steps: 50, guidance: 7, shift: 1, solver: 'euler', scheduler: 'linear' },
  merge: { steps: 60, guidance: 5, shift: 1, solver: 'euler', scheduler: 'linear' },
};

export const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 outline-hidden transition-colors focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/25 dark:text-white';
export const LABEL = 'mb-1.5 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400';
export const CHOICE = (on: boolean) => `rounded-lg border px-2 py-1.5 text-[11px] font-semibold transition-colors ${on
  ? 'border-pink-500 bg-pink-500/10 text-pink-600 dark:text-pink-300' : 'border-zinc-200 text-zinc-600 hover:border-pink-300 dark:border-white/10 dark:text-zinc-300'}`;

const numberOrUndefined = (value: unknown): number | undefined => {
  if (typeof value !== 'string' || value.trim() === '') return undefined;
  const parsed = Number(value.trim());
  return Number.isFinite(parsed) ? parsed : undefined;
};
const text = (draft: AceStudioDraft, key: string) => typeof draft[key] === 'string' ? draft[key] as string : '';

export function keptAceStudio(): AceStudioDraft {
  try {
    const kept: unknown = JSON.parse(window.localStorage.getItem(KEY) ?? '{}');
    return kept && typeof kept === 'object' ? kept as AceStudioDraft : {};
  } catch {
    return {};
  }
}

export function keepAceStudio(draft: AceStudioDraft) {
  try { window.localStorage.setItem(KEY, JSON.stringify(draft)); } catch { /* kept until a reload */ }
}

const FIELDS = ['synth_model', 'vocal_language', 'bpm', 'keyscale', 'timesignature', 'lm_temperature', 'lm_cfg_scale', 'lm_top_p', 'lm_top_k',
  'lm_rep_penalty', 'lm_rep_mode', 'lm_rep_window', 'lm_dry_base', 'lm_dry_min_len', 'lm_negative_prompt', 'audio_codes',
  'guidance', 'apg_momentum', 'apg_norm_threshold', 'dcw_mode', 'dcw_scaler', 'dcw_high_scaler', 'custom_timesteps',
  'latent_shift', 'latent_rescale', 'cfg_interval_start', 'cfg_interval_end', 'retake_variance', 'retake_seed', 'fade_in',
  'fade_out', 'shift', 'solver', 'scheduler', 'audio_cover_strength'];

/** A saved request's values back in the form, for Reuse. */
export function restoreAceStudio(settings: Record<string, unknown>): AceStudioDraft {
  const draft: AceStudioDraft = {};
  for (const key of FIELDS) {
    const value = settings[key];
    if (typeof value === 'number' || (typeof value === 'string' && value)) draft[key] = String(value);
  }
  if (typeof settings.think === 'boolean') draft.think = settings.think;
  if (typeof settings.use_cot_caption === 'boolean') draft.use_cot_caption = settings.use_cot_caption;
  const scales = settings.adapter_group_scales;
  if (scales && typeof scales === 'object') {
    for (const [group, value] of Object.entries(scales as Record<string, unknown>)) {
      if (typeof value === 'number') draft[`group_${group}`] = String(value);
    }
  }
  return draft;
}

/** The engine fields of the draft: only what differs from the engine's own choice. */
export function aceStudioFields(draft: AceStudioDraft, kind: DitKind, task: string): Record<string, unknown> {
  const fields: Record<string, unknown> = {};
  const put = (key: string, value: unknown) => { if (value !== undefined && value !== '') fields[key] = value; };
  const dit = DIT_DEFAULTS[kind];
  const think = draft.think !== false;
  fields.think = think;
  if (draft.use_cot_caption === true) fields.use_cot_caption = true;
  put('synth_model', text(draft, 'synth_model'));
  put('vocal_language', text(draft, 'vocal_language'));
  put('bpm', numberOrUndefined(draft.bpm));
  put('keyscale', text(draft, 'keyscale'));
  put('timesignature', text(draft, 'timesignature'));
  put('lm_temperature', numberOrUndefined(draft.lm_temperature));
  put('lm_cfg_scale', numberOrUndefined(draft.lm_cfg_scale));
  put('lm_top_p', numberOrUndefined(draft.lm_top_p));
  put('lm_top_k', numberOrUndefined(draft.lm_top_k));
  const penalty = numberOrUndefined(draft.lm_rep_penalty);
  if (penalty && penalty > 1) {
    put('lm_rep_penalty', penalty);
    put('lm_rep_mode', text(draft, 'lm_rep_mode'));
    put('lm_rep_window', numberOrUndefined(draft.lm_rep_window));
    if (draft.lm_rep_mode === 'dry') {
      put('lm_dry_base', numberOrUndefined(draft.lm_dry_base));
      put('lm_dry_min_len', numberOrUndefined(draft.lm_dry_min_len));
    }
  }
  put('lm_negative_prompt', text(draft, 'lm_negative_prompt').trim());
  const codes = readAudioCodes(text(draft, 'audio_codes'));
  if (codes.ok) put('audio_codes', codes.codes);
  put('shift', numberOrUndefined(draft.shift) ?? dit.shift);
  put('solver', text(draft, 'solver') || dit.solver);
  put('scheduler', text(draft, 'scheduler') || dit.scheduler);
  put('guidance', text(draft, 'guidance'));
  put('apg_momentum', numberOrUndefined(draft.apg_momentum));
  put('apg_norm_threshold', numberOrUndefined(draft.apg_norm_threshold));
  put('dcw_mode', text(draft, 'dcw_mode'));
  put('dcw_scaler', numberOrUndefined(draft.dcw_scaler));
  put('dcw_high_scaler', numberOrUndefined(draft.dcw_high_scaler));
  put('custom_timesteps', text(draft, 'custom_timesteps').trim());
  put('latent_shift', numberOrUndefined(draft.latent_shift));
  put('latent_rescale', numberOrUndefined(draft.latent_rescale));
  put('cfg_interval_start', numberOrUndefined(draft.cfg_interval_start));
  put('cfg_interval_end', numberOrUndefined(draft.cfg_interval_end));
  const variance = numberOrUndefined(draft.retake_variance);
  if (variance && variance > 0) {
    put('retake_variance', variance);
    put('retake_seed', numberOrUndefined(draft.retake_seed));
  }
  put('fade_in', numberOrUndefined(draft.fade_in));
  put('fade_out', numberOrUndefined(draft.fade_out));
  if (task === 'text2music' && think) put('audio_cover_strength', numberOrUndefined(draft.audio_cover_strength));
  const scales = Object.fromEntries(GROUPS.flatMap(group => {
    const value = numberOrUndefined(draft[`group_${group}`]);
    return value === undefined || value === 1 ? [] : [[group, value]];
  }));
  if (Object.keys(scales).length) fields.adapter_group_scales = scales;
  return fields;
}

/** The line ACE-Step reads the singer's voice from, added to the caption. */
export function aceGenderLine(draft: AceStudioDraft): string {
  return draft.gender === 'male' ? 'Male vocals' : draft.gender === 'female' ? 'Female vocals' : '';
}

export const Field: React.FC<{ label: string; hint?: string; children: React.ReactNode }> = ({ label, hint, children }) => (
  <label className="block">
    <span className={LABEL}>{label}</span>
    {children}
    {hint && <span className="mt-1 block text-[11px] leading-4 text-zinc-500">{hint}</span>}
  </label>
);

const Switch: React.FC<{ checked: boolean; onChange: (value: boolean) => void; label: string; hint?: string }> = ({ checked, onChange, label, hint }) => (
  <div className="flex items-center justify-between gap-3">
    <div className="min-w-0">
      <span className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</span>
      {hint && <p className="mt-0.5 text-[11px] leading-4 text-zinc-500">{hint}</p>}
    </div>
    <button type="button" role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)}
      className={`relative h-5 w-10 shrink-0 rounded-full transition-colors after:absolute after:inset-x-0 after:-inset-y-0.5 after:content-[''] ${checked ? 'bg-pink-500' : 'bg-zinc-300 dark:bg-zinc-600'}`}>
      <span className={`absolute top-[2px] h-4 w-4 rounded-full bg-white shadow-xs transition-all ${checked ? 'left-[22px]' : 'left-[2px]'}`} />
    </button>
  </div>
);

export const SliderRow: React.FC<{
  label: string; value: string; fallback: number; min: number; max: number; step: number; suffix?: string;
  onChange: (value: string) => void; disabled?: boolean;
}> = ({ label, value, fallback, min, max, step, suffix, onChange, disabled }) => {
  const current = value.trim() === '' ? fallback : Number(value);
  const shown = Number.isFinite(current) ? current : fallback;
  const decimals = step < 1 ? String(step).split('.')[1]?.length ?? 1 : 0;
  return (
    <div className={disabled ? 'opacity-50' : undefined}>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</span>
        <span className="flex items-center text-[11px] tabular-nums text-zinc-600 dark:text-zinc-300">
          <NumberField value={shown} min={min} max={max} step={step} label={label} disabled={disabled}
            format={number => `${number.toFixed(decimals)}${suffix ?? ''}`} onChange={number => onChange(String(number))} />
          {value.trim() === '' && <span className="ml-1 text-zinc-400">·</span>}
        </span>
      </div>
      <input type="range" min={min} max={max} step={step} value={shown} disabled={disabled} aria-label={label}
        onChange={event => onChange(event.target.value)} className="mt-1.5 h-1 w-full cursor-pointer accent-pink-500" />
    </div>
  );
};

const Stage: React.FC<{ title: string; hint: string; children: React.ReactNode }> = ({ title, hint, children }) => (
  <section>
    <h4 className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{title}</h4>
    <p className="mb-3 mt-0.5 text-[11px] leading-4 text-zinc-500">{hint}</p>
    {children}
  </section>
);

const PlanButton: React.FC<{ mode: 'inspire' | 'format'; label: string; disabled: boolean; planning: 'inspire' | 'format' | null; ready: boolean; onClick: () => void }> = ({ mode, label, disabled, planning, ready, onClick }) => (
  <button type="button" onClick={onClick} disabled={disabled || planning !== null || !ready}
    className="inline-flex min-h-6 items-center gap-1 rounded-md px-2 py-1 text-[10px] font-semibold text-pink-600 transition-colors hover:bg-pink-500/10 disabled:opacity-40 dark:text-pink-300">
    {planning === mode ? <Loader2 size={12} className="animate-spin" /> : mode === 'inspire' ? <Sparkles size={12} /> : <ListChecks size={12} />}
    {label}
  </button>
);

const UndoLink: React.FC<{ onClick: () => void }> = ({ onClick }) => {
  const { t } = useI18n();
  return <button type="button" onClick={onClick} className="mt-1.5 inline-flex items-center gap-1 text-[11px] font-semibold text-pink-600 hover:underline dark:text-pink-300">
    <Undo2 size={12} />{(t as unknown as (key: string) => string)('aceUndo')}
  </button>;
};

/** The idea a text-to-music song starts from: the language model writes the style, the lyrics and the metadata from it. */
export function AceSongIdea({ idea, onIdea, planning, ready, onPlan, onCreate, undo, onUndo, card }: {
  idea: string;
  onIdea: (idea: string) => void;
  /** Writes the song from the idea and creates it when the writing succeeds. */
  onCreate: (idea: string) => void;
  planning: 'inspire' | 'format' | null;
  ready: boolean;
  onPlan: (mode: 'inspire', idea: string) => void;
  undo: boolean;
  onUndo: () => void;
  card: (title: string, body: React.ReactNode, actions?: React.ReactNode) => React.ReactNode;
}) {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  return <>{card(tt('aceSongIdea'), <>
    <textarea value={idea} onChange={event => onIdea(event.target.value)} rows={2} placeholder={tt('aceIdeaPlaceholder')} aria-label={tt('aceSongIdea')}
      className={`${CONTROL} resize-y`} />
    <p className="mt-1.5 text-[11px] leading-4 text-zinc-500">{tt('aceIdeaHint')}</p>
    <div className="mt-2 flex flex-wrap items-center gap-2">
      <SecondaryButton disabled={!idea.trim() || planning !== null || !ready} onClick={() => onCreate(idea)}>
        {planning === 'inspire' ? <Loader2 size={12} className="animate-spin" /> : <Sparkles size={12} />}{tt('aceInspireCreate')}
      </SecondaryButton>
      {undo && <UndoLink onClick={onUndo} />}
    </div>
  </>, <PlanButton mode="inspire" label={planning === 'inspire' ? tt('acePlanning') : tt('aceInspire')} disabled={!idea.trim()}
    planning={planning} ready={ready} onClick={() => onPlan('inspire', idea)} />)}</>;
}

export function AceStudioFields({ value, onChange, kind, task, adapters, card, planning, ready, canFormat, onPlan, undo, onUndo }: {
  value: AceStudioDraft;
  onChange: (value: AceStudioDraft) => void;
  kind: DitKind;
  task: string;
  adapters: boolean;
  planning: 'inspire' | 'format' | null;
  ready: boolean;
  canFormat: boolean;
  onPlan: (mode: 'inspire' | 'format', idea?: string) => void;
  undo: 'inspire' | 'format' | null;
  onUndo: () => void;
  card: (title: string, body: React.ReactNode, actions?: React.ReactNode) => React.ReactNode;
}) {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [advanced, setAdvanced] = useState(false);
  const dit = DIT_DEFAULTS[kind];
  const turbo = kind === 'turbo';
  const set = (key: string) => (next: string | boolean) => onChange({ ...value, [key]: next });
  const get = (key: string) => text(value, key);
  const think = value.think !== false;
  const repMode = get('lm_rep_mode');
  const dcwMode = get('dcw_mode');
  const codes = get('audio_codes').trim();
  const codesRead = codes ? readAudioCodes(codes) : null;
  const codesProblem = !codesRead ? null
    : 'code' in codesRead ? tt('aceCodesForeign').replace('{code}', String(codesRead.code))
      : 'piece' in codesRead ? tt('aceCodesNotCodes').replace('{piece}', codesRead.piece) : null;
  const activePreset = PRESETS.find(preset => (numberOrUndefined(value.cfg_interval_start) ?? 0) === preset.start
    && (numberOrUndefined(value.cfg_interval_end) ?? 1) === preset.end && (get('guidance') || '') === preset.mode)?.id;
  const reset = () => onChange({ vocal_language: get('vocal_language'), gender: value.gender ?? '', bpm: get('bpm'), keyscale: get('keyscale'), timesignature: get('timesignature') });

  return <>
    {card(tt('aceSongParams'), <>
      <div className="grid grid-cols-2 gap-2">
        <Field label={tt('aceLanguage')}>
          <select value={get('vocal_language')} onChange={event => set('vocal_language')(event.target.value)} className={CONTROL}>
            {LANGUAGES.map(code => <option key={code} value={code}>{code ? tt(`aceLang_${code}`) : tt('aceAuto')}</option>)}
          </select>
        </Field>
        <Field label="BPM"><input value={get('bpm')} onChange={event => set('bpm')(event.target.value)} placeholder={tt('aceAuto')} inputMode="numeric" className={CONTROL} /></Field>
        <Field label={tt('aceKey')}>
          <select value={get('keyscale')} onChange={event => set('keyscale')(event.target.value)} className={CONTROL}>
            {KEYS.map(key => <option key={key} value={key}>{key || tt('aceAuto')}</option>)}
          </select>
        </Field>
        <Field label={tt('aceTimeSignature')}>
          <select value={get('timesignature')} onChange={event => set('timesignature')(event.target.value)} className={CONTROL}>
            {TIME_SIGNATURES.map(signature => <option key={signature} value={signature}>{signature ? (signature === '6' ? '6/8' : `${signature}/4`) : tt('aceAuto')}</option>)}
          </select>
        </Field>
      </div>
      <div className="mt-3">
        <span className={LABEL}>{tt('aceGender')}</span>
        <div className="flex gap-1.5">
          {(['', 'male', 'female'] as const).map(gender => (
            <button key={gender || 'any'} type="button" aria-pressed={(value.gender ?? '') === gender} onClick={() => set('gender')(gender)}
              className={`flex-1 ${CHOICE((value.gender ?? '') === gender)}`}>{tt(`aceGender_${gender || 'any'}`)}</button>
          ))}
        </div>
      </div>
      <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt('aceMetadataHint')}</p>
      {undo === 'format' && <UndoLink onClick={onUndo} />}
    </>, <PlanButton mode="format" label={tt('aceFormat')} disabled={!canFormat} planning={planning} ready={ready} onClick={() => onPlan('format')} />)}

    {card(t('quality'), <div className="space-y-3">
      <Switch checked={think} onChange={set('think')} label={tt('aceThink')} hint={tt('aceThinkHint')} />
      {think && task === 'text2music' && (
        <div>
          <SliderRow label={tt('acePlanStrength')} value={get('audio_cover_strength')} fallback={1} min={0} max={1} step={0.05} onChange={set('audio_cover_strength')} />
          <p className="mt-1 text-[11px] leading-4 text-zinc-500">{tt('acePlanStrengthHint')}</p>
        </div>
      )}
      {!turbo && (
        <div>
          <span className={LABEL}>{tt('acePresets')}</span>
          <div className="grid grid-cols-3 gap-1.5">
            {PRESETS.map(preset => (
              <button key={preset.id} type="button" title={tt(`acePresetHint_${preset.id}`)} aria-pressed={activePreset === preset.id}
                onClick={() => onChange({ ...value, cfg_interval_start: preset.start === 0 ? '' : String(preset.start), cfg_interval_end: preset.end === 1 ? '' : String(preset.end), guidance: preset.mode })}
                className={CHOICE(activePreset === preset.id)}>{tt(`acePreset_${preset.id}`)}</button>
            ))}
          </div>
          {activePreset && <p className="mt-1.5 text-[11px] leading-4 text-zinc-500">{tt(`acePresetHint_${activePreset}`)}</p>}
        </div>
      )}
      <p className="text-[11px] leading-4 text-zinc-500">{tt(`aceModelKind_${kind}`)}</p>
    </div>, <button type="button" onClick={reset} className="min-h-6 rounded-md px-2 py-1 text-[10px] font-semibold text-zinc-500 transition-colors hover:bg-zinc-200 hover:text-black dark:hover:bg-white/10 dark:hover:text-white">{t('resetToDefaults')}</button>)}

    <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-white/5 dark:bg-suno-card">
      <button type="button" aria-expanded={advanced} onClick={() => setAdvanced(current => !current)}
        className="flex w-full items-center justify-between gap-2 px-3 py-2 text-[11px] font-bold uppercase tracking-wide text-zinc-500 transition-colors hover:text-black dark:text-zinc-400 dark:hover:text-white">
        {t('advanced')}
        <ChevronDown size={15} className={advanced ? 'rotate-180 transition-transform' : 'transition-transform'} />
      </button>
      {advanced && (
        <div className="space-y-4 border-t border-zinc-100 p-3 dark:border-white/5">
          <Stage title={t('stageLm')} hint={tt('aceStageLmHint')}>
            <div className="space-y-3">
              <Switch checked={value.use_cot_caption === true} onChange={set('use_cot_caption')} label={tt('aceCotCaption')} hint={tt('aceCotCaptionHint')} />
              <SliderRow label={tt('aceTemperature')} value={get('lm_temperature')} fallback={0.85} min={0} max={2} step={0.05} onChange={set('lm_temperature')} />
              <SliderRow label={t('cfgScale')} value={get('lm_cfg_scale')} fallback={2} min={1} max={4} step={0.1} onChange={set('lm_cfg_scale')} />
              <SliderRow label="Top-P" value={get('lm_top_p')} fallback={0.9} min={0} max={1} step={0.01} onChange={set('lm_top_p')} />
              <SliderRow label={t('topK')} value={get('lm_top_k')} fallback={0} min={0} max={200} step={1} onChange={set('lm_top_k')} />
              <SliderRow label={tt('aceRepPenalty')} value={get('lm_rep_penalty')} fallback={1} min={1} max={1.5} step={0.01} onChange={set('lm_rep_penalty')} />
              <p className="-mt-1 text-[11px] leading-4 text-zinc-500">{tt('aceRepPenaltyHint')}</p>
              {(numberOrUndefined(value.lm_rep_penalty) ?? 1) > 1 && (
                <>
                  <Field label={tt('aceRepMode')}>
                    <select value={repMode || 'presence'} onChange={event => set('lm_rep_mode')(event.target.value)} className={CONTROL}>
                      {REP_MODES.map(mode => <option key={mode} value={mode}>{tt(`aceRepMode_${mode}`)}</option>)}
                    </select>
                  </Field>
                  <p className="-mt-1 text-[11px] leading-4 text-zinc-500">{tt(`aceRepModeHint_${repMode || 'presence'}`)}</p>
                  <SliderRow label={tt('aceRepWindow')} value={get('lm_rep_window')} fallback={64} min={8} max={256} step={8}
                    suffix={` · ${((numberOrUndefined(value.lm_rep_window) ?? 64) / 5).toFixed(1)} ${tt('aceSecondsShort')}`} onChange={set('lm_rep_window')} />
                  {repMode === 'dry' && (
                    <div className="grid grid-cols-2 gap-2">
                      <SliderRow label={tt('aceDryBase')} value={get('lm_dry_base')} fallback={1.75} min={1.05} max={4} step={0.05} onChange={set('lm_dry_base')} />
                      <SliderRow label={tt('aceDryMinLen')} value={get('lm_dry_min_len')} fallback={3} min={2} max={32} step={1} onChange={set('lm_dry_min_len')} />
                    </div>
                  )}
                </>
              )}
              <Field label={tt('aceNegative')}><input value={get('lm_negative_prompt')} onChange={event => set('lm_negative_prompt')(event.target.value)} className={CONTROL} /></Field>
              <Field label={tt('aceCodes')} hint={tt('aceCodesHint')}>
                <textarea value={get('audio_codes')} onChange={event => set('audio_codes')(event.target.value)} rows={2} aria-invalid={Boolean(codesProblem)} className={`${CONTROL} resize-y font-mono text-[11px]`} />
                {codesProblem && <span role="alert" className="mt-1 block text-[11px] leading-4 text-rose-600 dark:text-rose-300">{codesProblem}</span>}
              </Field>
            </div>
          </Stage>

          <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
            <Stage title={tt('aceStageDit')} hint={tt('aceStageDitHint')}>
              <div className="space-y-3">
                <SliderRow label={tt('aceShift')} value={get('shift')} fallback={dit.shift} min={1} max={5} step={0.1} onChange={set('shift')} />
                <div className="grid grid-cols-2 gap-2">
                  <Field label={tt('aceSolver')}>
                    <select value={get('solver') || dit.solver} onChange={event => set('solver')(event.target.value)} className={CONTROL}>
                      {SOLVERS.map(solver => <option key={solver} value={solver}>{tt(`aceSolver_${solver}`)}</option>)}
                    </select>
                  </Field>
                  <Field label={tt('aceScheduler')}>
                    <select value={get('scheduler') || dit.scheduler} onChange={event => set('scheduler')(event.target.value)} className={CONTROL}>
                      {SCHEDULERS.map(scheduler => <option key={scheduler} value={scheduler}>{tt(`aceScheduler_${scheduler}`)}</option>)}
                    </select>
                  </Field>
                </div>
                <p className="text-[11px] leading-4 text-zinc-500">{tt('aceSamplerHint')}</p>
                {!turbo && (
                  <>
                    <Field label={tt('aceGuidanceMode')}>
                      <select value={get('guidance') || 'apg'} onChange={event => set('guidance')(event.target.value)} className={CONTROL}>
                        {GUIDANCE.map(mode => <option key={mode} value={mode}>{tt(`aceGuidance_${mode}`)}</option>)}
                      </select>
                    </Field>
                    <div className="grid grid-cols-2 gap-2">
                      <SliderRow label={tt('aceApgMomentum')} value={get('apg_momentum')} fallback={0.75} min={0} max={0.99} step={0.01} onChange={set('apg_momentum')} />
                      <SliderRow label={tt('aceApgNorm')} value={get('apg_norm_threshold')} fallback={2.5} min={0} max={10} step={0.1} onChange={set('apg_norm_threshold')} />
                    </div>
                    <div className="grid grid-cols-2 gap-2">
                      <SliderRow label={tt('aceCfgStart')} value={get('cfg_interval_start')} fallback={0} min={0} max={1} step={0.05} onChange={set('cfg_interval_start')} />
                      <SliderRow label={tt('aceCfgEnd')} value={get('cfg_interval_end')} fallback={1} min={0} max={1} step={0.05} onChange={set('cfg_interval_end')} />
                    </div>
                    <p className="text-[11px] leading-4 text-zinc-500">{tt('aceCfgIntervalHint')}</p>
                  </>
                )}
                <div className="grid grid-cols-3 gap-2">
                  <Field label="DCW">
                    <select value={dcwMode} onChange={event => set('dcw_mode')(event.target.value)} className={CONTROL}>
                      <option value="">{tt('aceOff')}</option>
                      {['low', 'high', 'double', 'pix'].map(mode => <option key={mode} value={mode}>{mode}</option>)}
                    </select>
                  </Field>
                  <Field label={tt('aceDcwScaler')}><input value={get('dcw_scaler')} onChange={event => set('dcw_scaler')(event.target.value)} placeholder="0.1" inputMode="decimal" disabled={!dcwMode} className={CONTROL} /></Field>
                  <Field label={tt('aceDcwHigh')}><input value={get('dcw_high_scaler')} onChange={event => set('dcw_high_scaler')(event.target.value)} placeholder="0" inputMode="decimal" disabled={dcwMode !== 'double'} className={CONTROL} /></Field>
                </div>
                <p className="text-[11px] leading-4 text-zinc-500">{tt('aceDcwHint')}</p>
                <Field label={tt('aceTimesteps')} hint={tt('aceTimestepsHint')}>
                  <input value={get('custom_timesteps')} onChange={event => set('custom_timesteps')(event.target.value)} placeholder="0.97,0.76,0.615,0.5,0.395,0.28,0.18,0.085,0" className={`${CONTROL} font-mono text-[11px]`} />
                </Field>
                <SliderRow label={tt('aceRetake')} value={get('retake_variance')} fallback={0} min={0} max={1} step={0.01} onChange={set('retake_variance')} />
                {numberOrUndefined(value.retake_variance) ? (
                  <Field label={tt('aceRetakeSeed')}><input value={get('retake_seed')} onChange={event => set('retake_seed')(event.target.value)} placeholder={tt('aceRandom')} inputMode="numeric" className={CONTROL} /></Field>
                ) : null}
                <p className="text-[11px] leading-4 text-zinc-500">{tt('aceRetakeHint')}</p>
                <div className="grid grid-cols-2 gap-2">
                  <Field label={tt('aceLatentShift')}><input value={get('latent_shift')} onChange={event => set('latent_shift')(event.target.value)} placeholder="0" inputMode="decimal" className={CONTROL} /></Field>
                  <Field label={tt('aceLatentRescale')}><input value={get('latent_rescale')} onChange={event => set('latent_rescale')(event.target.value)} placeholder="1" inputMode="decimal" className={CONTROL} /></Field>
                </div>
              </div>
            </Stage>
          </div>

          {adapters && (
            <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
              <Stage title={tt('aceGroupScales')} hint={tt('aceGroupScalesHint')}>
                <div className="grid grid-cols-2 gap-3">
                  {GROUPS.map(group => (
                    <SliderRow key={group} label={tt(`aceGroup_${group}`)} value={get(`group_${group}`)} fallback={1} min={0} max={2} step={0.05} onChange={set(`group_${group}`)} />
                  ))}
                </div>
              </Stage>
            </div>
          )}

          <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
            <Stage title={t('stageOutput')} hint={t('stageOutputHint')}>
              <div className="grid grid-cols-2 gap-2">
                <Field label={tt('aceFadeIn')}><input value={get('fade_in')} onChange={event => set('fade_in')(event.target.value)} placeholder="0" inputMode="decimal" className={CONTROL} /></Field>
                <Field label={tt('aceFadeOut')}><input value={get('fade_out')} onChange={event => set('fade_out')(event.target.value)} placeholder="0" inputMode="decimal" className={CONTROL} /></Field>
              </div>
            </Stage>
          </div>
        </div>
      )}
    </div>
  </>;
}
