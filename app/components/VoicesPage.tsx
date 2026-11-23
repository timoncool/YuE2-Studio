import React, { useEffect, useId, useRef, useState } from 'react';
import { Mic, Trash2, Wand2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { rvcVoiceStrings } from '../i18n/rvcVoices';
import { voicesPageStrings } from '../i18n/voicesPage';
import { responseJson, type Voice, voiceName } from '../services/rvcVoices';
import { chooseLastRvcVoice, clearLastRvcVoice, useLastRvcVoice } from '../services/rvcQuick';
import { SELF_READ, useLibrarySongs } from '../services/studioQueries';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import Combobox from './kit/Combobox';
import { PickerOption, PickerPill } from './kit/PickerPill';
import { ConfirmDialog } from './ConfirmDialog';
import { RvcCuratedVoices } from './RvcCuratedVoices';
import { RvcApplyVoice } from './RvcApplyVoice';
import { SeedVcApply } from './SeedVcApply';
import { TrainingPanel } from './TrainingPanel';

const URL = '/v1/rvc/voices';
const CARD = 'rounded-2xl border border-zinc-200 bg-white p-4 dark:border-white/10 dark:bg-white/[0.03]';
const CONTROL = 'block w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 focus:border-pink-500 focus:outline-none disabled:opacity-50 dark:border-white/10 dark:bg-black/25 dark:text-white';

type Tab = 'installed' | 'catalog' | 'apply' | 'training';

/** The RVC section: installed voices, the catalogue, putting a voice on a track and training one, like the LoRA page. */
export function VoicesPage({ initialSongId, onSaved }: { initialSongId?: string; onSaved?: () => void }) {
  const { language } = useI18n();
  const text = voicesPageStrings[language];
  const labels = rvcVoiceStrings[language];
  const [tab, setTab] = useState<Tab>(initialSongId ? 'apply' : 'installed');
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<Voice | null>(null);
  const selected = useLastRvcVoice();
  const voice = voices?.find(item => item.id === selected);

  useEffect(() => {
    const controller = new AbortController();
    void fetch(URL, { signal: controller.signal }).then(responseJson<Voice[]>)
      .then(list => { if (!controller.signal.aborted) { setVoices(list); setError(null); } })
      .catch((failure: Error) => { if (!controller.signal.aborted) setError(failure.message); });
    return () => controller.abort();
  }, [reload]);

  const acceptVoice = (installed: Voice) => {
    setVoices(current => [...(current ?? []).filter(item => item.id !== installed.id), installed]
      .sort((a, b) => voiceName(a).localeCompare(voiceName(b), language)));
    chooseLastRvcVoice(installed.id);
  };
  const chooseInstalled = (id: string) => {
    chooseLastRvcVoice(id);
    if (!voices?.some(item => item.id === id)) setReload(count => count + 1);
  };
  async function removeVoice(target: Voice) {
    setRemoving(null);
    setBusy(true);
    setError(null);
    try {
      await responseJson(await fetch(`${URL}/${encodeURIComponent(target.id)}`, { method: 'DELETE' }));
      setVoices(current => current?.filter(item => item.id !== target.id) ?? null);
      clearLastRvcVoice(target.id);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  }

  const tabs: [Tab, string][] = [['installed', `${text.installed} · ${voices?.length ?? 0}`], ['catalog', text.catalog], ['apply', text.apply], ['training', text.training]];
  return (
    <div className="flex-1 overflow-y-auto bg-white px-5 py-6 dark:bg-suno md:px-8">
      <div className="mx-auto max-w-4xl space-y-6">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.18em] text-pink-500">{text.eyebrow}</p>
          <h1 className="mt-1 text-2xl font-bold text-zinc-950 dark:text-white">{text.heading}</h1>
          <p className="mt-2 max-w-3xl text-sm text-zinc-600 dark:text-zinc-400">{text.intro}</p>
        </div>

        <div role="tablist" className="flex rounded-lg bg-zinc-100 p-1 dark:bg-white/5">
          {tabs.map(([value, label]) => (
            <button key={value} type="button" role="tab" aria-selected={tab === value} onClick={() => setTab(value)}
              className={`min-h-9 flex-1 rounded-md px-2 py-1.5 text-xs font-semibold transition-all max-md:min-h-11 ${tab === value ? 'bg-white text-black shadow-xs dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}>
              {label}
            </button>
          ))}
        </div>

        {error && <div role="alert" className="flex flex-wrap items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">
          <span className="min-w-0 flex-1 break-words">{error}</span>
          {!voices && <SecondaryButton onClick={() => setReload(count => count + 1)}>{labels.retry}</SecondaryButton>}
        </div>}

        {tab === 'installed' && <div className="space-y-4">
          {voices && voices.length === 0 && <div className={`${CARD} text-sm text-zinc-600 dark:text-zinc-300`}>
            <p>{text.empty}</p>
            <PrimaryButton className="mt-3" onClick={() => setTab('catalog')}>{text.openCatalog}</PrimaryButton>
          </div>}
          {voices && voices.length > 0 && <ul className={`${CARD} divide-y divide-zinc-100 p-0 dark:divide-white/5`}>
            {voices.map(item => <li key={item.id} className="flex items-center gap-3 px-4 py-3">
              <span className="flex size-10 shrink-0 items-center justify-center rounded-md bg-pink-500/10 text-pink-600 dark:text-pink-300"><Mic size={18} aria-hidden /></span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-semibold text-zinc-900 dark:text-white">{voiceName(item)}</span>
                <span className="block truncate text-xs text-zinc-500 dark:text-zinc-400">
                  {item.model_name}{item.version ? ` · RVC v${item.version}` : ''} · {item.index_name ? text.withIndex : text.withoutIndex}
                </span>
              </span>
              <SecondaryButton disabled={busy} className="shrink-0" title={text.applyVoice} onClick={() => { chooseLastRvcVoice(item.id); setTab('apply'); }}>
                <Wand2 size={14} aria-hidden /><span className="max-sm:sr-only">{text.applyVoice}</span>
              </SecondaryButton>
              <button type="button" disabled={busy} onClick={() => setRemoving(item)} aria-label={`${labels.remove}: ${voiceName(item)}`} title={labels.remove}
                className="inline-flex size-9 shrink-0 items-center justify-center rounded-lg text-zinc-500 max-md:size-11 hover:bg-rose-500/10 hover:text-rose-600 disabled:opacity-50 dark:text-zinc-400">
                <Trash2 size={16} aria-hidden />
              </button>
            </li>)}
          </ul>}
          <OwnVoiceFiles disabled={busy || !voices} onImported={acceptVoice} onError={setError} />
        </div>}

        {tab === 'catalog' && <RvcCuratedVoices onInstalled={acceptVoice} onChoose={chooseInstalled} disabled={!voices || busy}
          installedIds={new Set(voices?.map(item => item.id))} />}

        {tab === 'apply' && <ApplyToTrack voices={voices ?? []} voice={voice} initialSongId={initialSongId} onSaved={onSaved}
          onOpenCatalog={() => setTab('catalog')} />}

        {tab === 'training' && <TrainingPanel voices />}

        {removing && <ConfirmDialog isOpen title={labels.remove} confirmLabel={labels.remove}
          message={labels.confirmRemove.replace('{name}', () => voiceName(removing))}
          onConfirm={() => void removeVoice(removing)} onCancel={() => setRemoving(null)} />}
      </div>
    </div>
  );
}

function OwnVoiceFiles({ disabled, onImported, onError }: { disabled: boolean; onImported: (voice: Voice) => void; onError: (error: string) => void }) {
  const { language } = useI18n();
  const labels = rvcVoiceStrings[language];
  const [model, setModel] = useState<File>();
  const [index, setIndex] = useState<File>();
  const [busy, setBusy] = useState(false);
  const modelInput = useRef<HTMLInputElement>(null);
  const indexInput = useRef<HTMLInputElement>(null);
  const modelHelpId = useId();
  const indexHelpId = useId();
  async function importVoice() {
    if (!model || busy) return;
    setBusy(true);
    try {
      const form = new FormData();
      form.append('model', model);
      if (index) form.append('index', index);
      onImported(await responseJson<Voice>(await fetch(`${URL}/import`, { method: 'POST', body: form })));
      setModel(undefined);
      setIndex(undefined);
      if (modelInput.current) modelInput.current.value = '';
      if (indexInput.current) indexInput.current.value = '';
    } catch (failure) {
      onError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className={`${CARD} text-xs`}>
      <summary className="min-h-6 cursor-pointer text-sm font-semibold">{labels.ownFiles}</summary>
      <div className="mt-3 grid gap-2 sm:grid-cols-2">
        <div className="space-y-1">
          <label className="block space-y-1">
            <span>{labels.model}</span>
            <input ref={modelInput} className={CONTROL} type="file" accept=".pth,.pt" disabled={disabled || busy} aria-describedby={modelHelpId}
              onChange={event => setModel(event.target.files?.[0])} />
          </label>
          <p id={modelHelpId} className="text-zinc-600 dark:text-zinc-300">{labels.modelHelp}</p>
        </div>
        <div className="space-y-1">
          <label className="block space-y-1">
            <span>{labels.index}</span>
            <input ref={indexInput} className={CONTROL} type="file" accept=".index" disabled={disabled || busy} aria-describedby={indexHelpId}
              onChange={event => setIndex(event.target.files?.[0])} />
          </label>
          <p id={indexHelpId} className="text-zinc-600 dark:text-zinc-300">{labels.indexHelp}</p>
        </div>
      </div>
      <SecondaryButton className="mt-3" onClick={() => void importVoice()} disabled={!model || busy || disabled}>
        {busy ? labels.importing : labels.import}
      </SecondaryButton>
    </details>
  );
}

function ApplyToTrack({ voices, voice, initialSongId, onSaved, onOpenCatalog }: {
  voices: Voice[]; voice?: Voice; initialSongId?: string; onSaved?: () => void; onOpenCatalog: () => void;
}) {
  const { language } = useI18n();
  const text = voicesPageStrings[language];
  const labels = rvcVoiceStrings[language];
  const library = useLibrarySongs();
  const songs = library.data ?? [];
  const [songId, setSongId] = useState(initialSongId ?? '');
  const [stems, setStems] = useState<string[] | null>(null);
  const [stemsError, setStemsError] = useState<string | null>(null);
  useEffect(() => {
    if (!songId) { setStems(null); return; }
    const controller = new AbortController();
    setStems(null);
    void fetch(`/v1/library/songs/${encodeURIComponent(songId)}/stems`, { headers: SELF_READ, signal: controller.signal })
      .then(responseJson<{ stems: string[] }>)
      .then(body => { if (!controller.signal.aborted) { setStems(body.stems); setStemsError(null); } })
      .catch((failure: Error) => { if (!controller.signal.aborted) setStemsError(failure.message); });
    return () => controller.abort();
  }, [songId]);
  const song = songs.find(entry => entry.id === songId);
  return (
    <div className={`${CARD} space-y-3`}>
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1 text-xs">
          <span className="font-semibold text-zinc-600 dark:text-zinc-300">{text.track}</span>
          {library.isSuccess && songs.length === 0 ? <p className="text-zinc-500">{text.noTracks}</p>
            : <Combobox label={text.track} value={songId} placeholder={text.chooseTrack} onChange={setSongId}
              inputClassName="min-h-9 w-full rounded-lg border border-zinc-200 bg-zinc-50 px-2.5 py-2 text-sm font-semibold text-zinc-900 placeholder:font-normal placeholder:text-zinc-500 focus:border-pink-500 focus:outline-none dark:border-white/10 dark:bg-black/25 dark:text-white max-md:min-h-11"
              options={songs.map(entry => ({ value: entry.id, label: entry.title || '—', hint: entry.style?.slice(0, 80) }))} />}
        </div>
        <div className="space-y-1 text-xs">
          <span className="font-semibold text-zinc-600 dark:text-zinc-300">{labels.voice}</span>
          <PickerPill label={labels.voice} showLabel={false} value={voice ? voiceName(voice) : labels.none}>
            {close => <>
              {voices.map(item => <PickerOption key={item.id} value={item.id} selected={item.id === voice?.id} name={voiceName(item)}
                hint={`${item.model_name} · ${item.index_name ? text.withIndex : text.withoutIndex}`}
                onPick={() => { chooseLastRvcVoice(item.id); close(); }} />)}
              <div role="separator" className="my-1 border-t border-zinc-200 dark:border-white/10" />
              <PickerOption action selected={false} name={`＋ ${text.addVoice}`} hint={text.openCatalog} onPick={() => { close(); onOpenCatalog(); }} />
            </>}
          </PickerPill>
        </div>
      </div>
      {stemsError && <p role="alert" className="text-xs text-rose-600 dark:text-rose-300">{stemsError}</p>}
      {song && stems && <RvcApplyVoice key={song.id} song={{ id: song.id, title: song.title, stems }} voice={voice} control={CONTROL} onSaved={onSaved} />}
      {song && stems && <details className="border-t border-zinc-200 pt-3 text-xs dark:border-white/10">
        <summary className="min-h-6 cursor-pointer text-sm font-semibold">{labels.seedVc}</summary>
        <SeedVcApply key={`seed-vc:${song.id}`} song={{ id: song.id, title: song.title, stems }} control={CONTROL} onSaved={onSaved} />
      </details>}
    </div>
  );
}
