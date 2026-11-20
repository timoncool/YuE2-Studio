import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { rvcVoiceStrings } from '../i18n/rvcVoices';
import { responseJson, type Voice, voiceName } from '../services/rvcVoices';
import { RvcCuratedVoices } from './RvcCuratedVoices';

const URL = '/v1/rvc/voices';
const STORAGE = 'studio.rvc.voiceId';

export function RvcVoiceCatalog({ card, control }: { card: string; control: string }) {
  const { language } = useI18n();
  const labels = rvcVoiceStrings[language];
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const [selected, setSelected] = useState(() => localStorage.getItem(STORAGE) ?? '');
  const [model, setModel] = useState<File>();
  const [index, setIndex] = useState<File>();
  const modelInput = useRef<HTMLInputElement>(null);
  const indexInput = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
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
    setSelected(id);
    localStorage.setItem(STORAGE, id);
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

  const voice = voices?.find(item => item.id === selected);
  return (
    <section className={card} aria-label={labels.title}>
      <h2 className="text-sm font-semibold">{labels.title}</h2>
      <label className="mt-2 block space-y-1 text-xs">
        <span>{labels.choose}</span>
        <select className={control} value={voice?.id ?? ''} disabled={!voices?.length || busy}
          onChange={event => choose(event.target.value)}>
          <option value="">{labels.none}</option>
          {voices?.map(item => (
            <option key={item.id} value={item.id}>{voiceName(item)}{item.version ? ` · v${item.version}` : ''}{item.index_name ? ` · ${item.index_name}` : ''}</option>
          ))}
        </select>
      </label>
      <div className="mt-3 grid gap-2 sm:grid-cols-2">
        <label className="space-y-1 text-xs">
          <span>{labels.model}</span>
          <input ref={modelInput} className={control} type="file" accept=".pth,.pt" disabled={busy}
            onChange={event => setModel(event.target.files?.[0])} />
        </label>
        <label className="space-y-1 text-xs">
          <span>{labels.index}</span>
          <input ref={indexInput} className={control} type="file" accept=".index" disabled={busy}
            onChange={event => setIndex(event.target.files?.[0])} />
        </label>
      </div>
      <button type="button" onClick={() => void importVoice()} disabled={!model || busy || !voices}
        className="mt-3 rounded-lg border border-zinc-200 px-3 py-2 text-xs enabled:hover:border-pink-500 disabled:opacity-50 dark:border-white/10">
        {busy ? labels.importing : labels.import}
      </button>
      {error && <p role="alert" className="mt-2 break-words text-xs text-rose-600 dark:text-rose-300">{error}</p>}
      {!voices && error && (
        <button type="button" className="mt-2 text-xs text-pink-600 underline dark:text-pink-300"
          onClick={() => { setError(null); setCatalogRequest(request => request + 1); }}>
          {labels.retry}
        </button>
      )}
      <RvcCuratedVoices onInstalled={acceptVoice} onChoose={chooseInstalled} disabled={!voices || busy} />
    </section>
  );
}
