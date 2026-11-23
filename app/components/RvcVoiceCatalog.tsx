import { useEffect, useId, useRef, useState } from 'react';
import { Mic } from 'lucide-react';
import { SecondaryButton } from './kit/Buttons';
import { useI18n } from '../context/I18nContext';
import { rvcVoiceStrings } from '../i18n/rvcVoices';
import { responseJson, type Voice, voiceName } from '../services/rvcVoices';
import { chooseLastRvcVoice, clearLastRvcVoice, useLastRvcVoice } from '../services/rvcQuick';
import { ConfirmDialog } from './ConfirmDialog';
import { RvcCuratedVoices } from './RvcCuratedVoices';
import { RvcApplyVoice, type RvcSourceSong } from './RvcApplyVoice';
import { SeedVcApply } from './SeedVcApply';

const URL = '/v1/rvc/voices';

export function RvcVoiceCatalog({ card, control, song, onSaved }: { card: string; control: string; song?: RvcSourceSong; onSaved?: () => void }) {
  const { language } = useI18n();
  const labels = rvcVoiceStrings[language];
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const selected = useLastRvcVoice();
  const [model, setModel] = useState<File>();
  const [index, setIndex] = useState<File>();
  const modelInput = useRef<HTMLInputElement>(null);
  const indexInput = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<Voice | null>(null);
  const modelHelpId = useId();
  const indexHelpId = useId();
  const [catalogRequest, setCatalogRequest] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    void fetch(URL, { signal: controller.signal }).then(responseJson<Voice[]>)
      .then(list => { if (!controller.signal.aborted) setVoices(list); })
      .catch((failure: Error) => { if (!controller.signal.aborted) setError(failure.message); });
    return () => controller.abort();
  }, [catalogRequest]);

  function choose(id: string) {
    chooseLastRvcVoice(id);
  }

  function chooseInstalled(id: string) {
    choose(id);
    if (!voices?.some(voice => voice.id === id)) setCatalogRequest(request => request + 1);
  }

  function acceptVoice(voice: Voice) {
    setVoices(current => [...(current ?? []).filter(item => item.id !== voice.id), voice]
      .sort((a, b) => voiceName(a).localeCompare(voiceName(b), language)));
    choose(voice.id);
  }

  async function importVoice() {
    if (!model || busy) return;
    setBusy(true);
    setError(null);
    try {
      const form = new FormData();
      form.append('model', model);
      if (index) form.append('index', index);
      const voice = await responseJson<Voice>(await fetch(`${URL}/import`, { method: 'POST', body: form }));
      acceptVoice(voice);
      setModel(undefined);
      setIndex(undefined);
      if (modelInput.current) modelInput.current.value = '';
      if (indexInput.current) indexInput.current.value = '';
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  }

  async function removeVoice(voice: Voice) {
    setRemoving(null);
    setBusy(true);
    setError(null);
    try {
      await responseJson(await fetch(`${URL}/${encodeURIComponent(voice.id)}`, { method: 'DELETE' }));
      setVoices(current => current?.filter(item => item.id !== voice.id) ?? null);
      clearLastRvcVoice(voice.id);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  }

  const voice = voices?.find(item => item.id === selected);
  return (
    <section className={card} aria-label={labels.title}>
      <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
        <Mic size={17} className="text-pink-500" /> {labels.title}
      </div>
      <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{labels.intro}</p>
      <p className="mt-2 text-xs text-zinc-500 dark:text-zinc-400">
        {song ? `${labels.song}: ${song.title || '—'}` : labels.noSong}
        {voice && ` · ${labels.voice}: ${voiceName(voice)}`}
      </p>
      {!!voices?.length && (
        <div className="mt-3 flex flex-wrap items-end gap-2">
          <label className="block min-w-48 flex-1 space-y-1 text-xs">
            <span>{labels.choose}</span>
            <select className={control} value={voice?.id ?? ''} disabled={busy} onChange={event => choose(event.target.value)}>
              <option value="">{labels.none}</option>
              {voices.map(item => <option key={item.id} value={item.id}>{voiceName(item)}</option>)}
            </select>
          </label>
          {voice && <SecondaryButton disabled={busy} onClick={() => setRemoving(voice)}>{labels.remove}</SecondaryButton>}
        </div>
      )}
      <RvcCuratedVoices onInstalled={acceptVoice} onChoose={chooseInstalled} disabled={!voices || busy}
        installedIds={new Set(voices?.map(item => item.id))} />
      {song && <RvcApplyVoice key={song.id} song={song} voice={voice} control={control} onSaved={onSaved} />}
      {removing && <ConfirmDialog isOpen title={labels.remove} confirmLabel={labels.remove}
        message={labels.confirmRemove.replace('{name}', () => voiceName(removing))}
        onConfirm={() => void removeVoice(removing)} onCancel={() => setRemoving(null)} />}
      {error && <p role="alert" className="mt-2 break-words text-xs text-rose-600 dark:text-rose-300">{error}</p>}
      {!voices && error && (
        <SecondaryButton className="mt-2" onClick={() => { setError(null); setCatalogRequest(request => request + 1); }}>
          {labels.retry}
        </SecondaryButton>
      )}
      <details className="mt-4 border-t border-zinc-200 pt-3 text-xs dark:border-white/10">
        <summary className="cursor-pointer text-sm font-semibold">{labels.ownFiles}</summary>
        <div className="mt-3 grid gap-2 sm:grid-cols-2">
          <div className="space-y-1">
            <label className="block space-y-1">
              <span>{labels.model}</span>
              <input ref={modelInput} className={control} type="file" accept=".pth,.pt" disabled={busy} aria-describedby={modelHelpId}
                onChange={event => setModel(event.target.files?.[0])} />
            </label>
            <p id={modelHelpId} className="text-zinc-600 dark:text-zinc-300">{labels.modelHelp}</p>
          </div>
          <div className="space-y-1">
            <label className="block space-y-1">
              <span>{labels.index}</span>
              <input ref={indexInput} className={control} type="file" accept=".index" disabled={busy} aria-describedby={indexHelpId}
                onChange={event => setIndex(event.target.files?.[0])} />
            </label>
            <p id={indexHelpId} className="text-zinc-600 dark:text-zinc-300">{labels.indexHelp}</p>
          </div>
        </div>
        <SecondaryButton className="mt-3" onClick={() => void importVoice()} disabled={!model || busy || !voices}>
          {busy ? labels.importing : labels.import}
        </SecondaryButton>
      </details>
      {song && (
        <details className="mt-3 border-t border-zinc-200 pt-3 text-xs dark:border-white/10">
          <summary className="cursor-pointer text-sm font-semibold">{labels.seedVc}</summary>
          <SeedVcApply key={`seed-vc:${song.id}`} song={song} control={control} onSaved={onSaved} />
        </details>
      )}
    </section>
  );
}
