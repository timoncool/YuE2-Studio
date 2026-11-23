import { useEffect, useRef, useState } from 'react';
import { Play } from 'lucide-react';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import { WorkProgress } from './WorkProgress';
import { useI18n } from '../context/I18nContext';
import { rvcVoiceStrings } from '../i18n/rvcVoices';
import { apiUrl } from '../services/apiBase';
import { poll } from '../services/poll';
import { responseJson, type Voice, voiceName } from '../services/rvcVoices';

const URL = '/v1/rvc/catalog';
const PAGE_SIZE = 40;

interface CatalogVoice {
  id: string;
  name: string;
  source_repo: string;
  source_url: string;
  license: string | null;
  weights?: { size: number };
  index?: { size: number } | null;
  download_bytes?: number;
  has_index?: boolean;
  sample_audio_url?: string | null;
  installed_id: string | null;
  metadata?: {
    source_labels?: string[] | null;
    vocal_style?: string | null;
    rvc_version?: string | number | null;
    sample_rate_hz?: number | null;
  } | null;
}

const metadataFields = ['vocal_style', 'rvc_version', 'sample_rate_hz'] as const;
type MetadataField = typeof metadataFields[number];
type Filters = Partial<Record<MetadataField | 'index' | 'sample', string>>;
interface InstallProgress {
  stage: 'preparing' | 'downloading_weights' | 'downloading_archive' | 'downloading_index' | 'verifying';
  downloaded_bytes: number;
  total_bytes: number;
}

export function RvcCuratedVoices({ onInstalled, onChoose, disabled, installedIds }: {
  installedIds: ReadonlySet<string>;
  onInstalled: (voice: Voice) => void;
  onChoose: (id: string) => void;
  disabled: boolean;
}) {
  const { language } = useI18n();
  const labels = rvcVoiceStrings[language];
  const [playingSample, setPlayingSample] = useState<string>();
  const [sampleError, setSampleError] = useState(false);
  const [catalog, setCatalog] = useState<CatalogVoice[] | null>(null);
  const [request, setRequest] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshed, setRefreshed] = useState(false);
  const [installing, setInstalling] = useState<string>();
  const [error, setError] = useState<string>();
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [installedName, setInstalledName] = useState<string>();
  const [search, setSearch] = useState('');
  const [filters, setFilters] = useState<Filters>({});
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const sizeFormat = new Intl.NumberFormat(language, { style: 'unit', unit: 'megabyte', maximumFractionDigits: 1 });

  function setFilter(field: MetadataField | 'index' | 'sample', value: string) {
    setFilters(current => ({ ...current, [field]: value }));
    setVisibleCount(PAGE_SIZE);
  }

  useEffect(() => {
    const controller = new AbortController();
    void fetch(URL, { signal: controller.signal }).then(responseJson<CatalogVoice[]>)
      .then(items => { if (!controller.signal.aborted) setCatalog(items); })
      .catch((failure: Error) => { if (!controller.signal.aborted) setError(failure.message); });
    return () => controller.abort();
  }, [request]);

  useEffect(() => {
    if (!installing) return;
    const controller = new AbortController();
    const stop = poll(async () => {
      try {
        const reply = await fetch(`${URL}/progress/${encodeURIComponent(installing)}`, { signal: controller.signal });
        const next = await responseJson<InstallProgress | null>(reply);
        if (next && !controller.signal.aborted) setProgress(next);
      } catch {
        // The install request reports the final result. A missed progress sample can be retried.
      }
    }, 350);
    return () => { controller.abort(); stop(); };
  }, [installing]);

  async function refresh() {
    setRefreshing(true);
    setRefreshed(false);
    setError(undefined);
    try {
      await responseJson(await fetch(`${URL}/refresh`, { method: 'POST' }));
      setRequest(value => value + 1);
      setRefreshed(true);
    } catch (failure) {
      setError(`${labels.refreshFailed} ${failure instanceof Error ? failure.message : String(failure)}`);
    } finally {
      setRefreshing(false);
    }
  }

  async function install(id: string) {
    setInstalling(id);
    setProgress(null);
    setInstalledName(undefined);
    setError(undefined);
    try {
      const voice = await responseJson<Voice>(await fetch(`${URL}/install`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id }),
      }));
      setCatalog(current => current?.map(item => item.id === id ? { ...item, installed_id: voice.id } : item) ?? null);
      onInstalled(voice);
      setInstalledName(voiceName(voice));
    } catch (failure) {
      setError(`${labels.installFailed} ${failure instanceof Error ? failure.message : String(failure)}`);
    } finally {
      setInstalling(undefined);
    }
  }

  const terms = search.trim().toLocaleLowerCase(language).split(/\s+/).filter(Boolean);
  const matches = (catalog ?? []).filter(voice => {
    const searchable = [voice.name, ...(voice.metadata?.source_labels ?? [])].join(' ').toLocaleLowerCase(language);
    return terms.every(term => searchable.includes(term))
      && metadataFields.every(field => !filters[field] || String(voice.metadata?.[field] ?? '') === filters[field])
      && (!filters.index || ((voice.has_index ?? !!voice.index) ? 'present' : 'absent') === filters.index)
      && (!filters.sample || !!voice.sample_audio_url);
  });
  const visible = matches.slice(0, visibleCount);
  const filterActive = terms.length > 0 || Object.values(filters).some(Boolean);
  const stageLabels = {
    preparing: labels.preparing,
    downloading_weights: labels.fetchingWeights,
    downloading_archive: labels.fetchingArchive,
    downloading_index: labels.fetchingIndex,
    verifying: labels.verifying,
  };
  const activeStage = installing ? stageLabels[progress?.stage ?? 'preparing'] : undefined;
  const filterLabels = {
    vocal_style: labels.vocalStyle,
    rvc_version: labels.rvcVersion,
    sample_rate_hz: labels.sampleRate,
  };

  const styles = [...new Set((catalog ?? []).flatMap(voice => voice.metadata?.vocal_style ? [voice.metadata.vocal_style] : []))]
    .sort((a, b) => a.localeCompare(b, language));
  const samples = (catalog ?? []).some(voice => voice.sample_audio_url);
  return (
    <div className="mt-4 border-t border-zinc-200 pt-3 text-xs dark:border-white/10">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">{labels.curated}</h3>
        <SecondaryButton disabled={refreshing || Boolean(installing)} onClick={() => void refresh()}>
          {refreshing ? labels.refreshing : labels.refresh}
        </SecondaryButton>
      </div>
      {refreshed && <p role="status" className="mt-1">{labels.refreshed}</p>}
      {!catalog && !error && <p role="status" className="mt-2">{labels.loading}</p>}
      {catalog?.length === 0 && <p className="mt-2">{labels.empty}</p>}
      {error && <p role="alert" className="mt-2 break-words text-rose-600 dark:text-rose-300">{error}</p>}
      {!installing && installedName && <p role="status" className="mt-2">{labels.installDone.replace('{voice}', installedName)}</p>}
      {!catalog && error && (
        <SecondaryButton className="mt-2" onClick={() => { setError(undefined); setRequest(value => value + 1); }}>{labels.retry}</SecondaryButton>
      )}
      {catalog && catalog.length > 0 && (
        <>
          <div className="mt-2 flex flex-wrap items-end gap-2">
            <label className="block min-w-48 flex-1 space-y-1">
              <span>{labels.search}</span>
              <input type="search" value={search} onChange={event => { setSearch(event.target.value); setVisibleCount(PAGE_SIZE); }}
                className="w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm dark:border-white/10 dark:bg-black/30" />
            </label>
            {styles.length > 1 && (
              <label className="block space-y-1">
                <span>{labels.vocalStyle}</span>
                <select value={filters.vocal_style ?? ''} onChange={event => setFilter('vocal_style', event.target.value)}
                  className="rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm dark:border-white/10 dark:bg-black/30">
                  <option value="">{labels.all}</option>
                  {styles.map(value => <option key={value} value={value}>{value}</option>)}
                </select>
              </label>
            )}
            {samples && (
              <label className="flex items-center gap-1.5 pb-2">
                <input type="checkbox" checked={!!filters.sample} onChange={event => {
                  setFilters(current => ({ ...current, sample: event.target.checked ? 'present' : undefined }));
                  setVisibleCount(PAGE_SIZE);
                }} />
                {labels.withSample}
              </label>
            )}
          </div>
          <p role="status" className="mt-2 text-zinc-500 dark:text-zinc-400">
            {matches.length === 0 ? labels.noMatch : labels.resultCount
              .replace('{visible}', visible.length.toLocaleString(language))
              .replace('{total}', matches.length.toLocaleString(language))}
            {filterActive && <button type="button" className="ml-2 font-medium hover:text-pink-600" onClick={() => { setSearch(''); setFilters({}); setVisibleCount(PAGE_SIZE); }}>{labels.clear}</button>}
          </p>
          <ul className="mt-1 max-h-[28rem] divide-y divide-zinc-200 overflow-auto dark:divide-white/10">
            {visible.map(voice => {
              const installedId = voice.installed_id && installedIds.has(voice.installed_id) ? voice.installed_id : null;
              const size = (voice.download_bytes ?? ((voice.weights?.size ?? 0) + (voice.index?.size ?? 0))) / 1_000_000;
              const percent = installing === voice.id && progress && progress.total_bytes > 0
                ? Math.min(100, Math.floor(100 * progress.downloaded_bytes / progress.total_bytes)) : null;
              return (
                <li key={voice.id} className="py-2.5">
                  <div className="flex flex-wrap items-center gap-2">
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{voice.name}</p>
                      <p className="mt-0.5 text-zinc-500 dark:text-zinc-400">
                        {voice.metadata?.vocal_style ? `${voice.metadata.vocal_style} · ` : ''}{voice.license ?? labels.licenseUnknown}
                        {' · '}{sizeFormat.format(size)}{' · '}
                        <a href={voice.source_url} target="_blank" rel="noreferrer" className="hover:text-pink-600">{voice.source_repo}</a>
                      </p>
                    </div>
                    {voice.sample_audio_url && (playingSample === voice.id && !sampleError
                      ? <SourceDemo key={voice.id} url={apiUrl(voice.sample_audio_url)} name={voice.name} onError={() => setSampleError(true)} />
                      : <SecondaryButton aria-label={`${labels.playSample} ${voice.name}`}
                          onClick={() => { setSampleError(false); setPlayingSample(voice.id); }}><Play size={12} />{labels.playSample}</SecondaryButton>)}
                    {installedId
                      ? <SecondaryButton disabled={disabled} aria-label={`${labels.selectInstalled} ${voice.name}`} onClick={() => onChoose(installedId)}>{labels.selectInstalled}</SecondaryButton>
                      : installing !== voice.id && <PrimaryButton className="px-3 py-1.5 text-xs" disabled={disabled || refreshing || Boolean(installing)}
                          aria-label={`${labels.install} ${voice.name}`} onClick={() => void install(voice.id)}>{labels.install}</PrimaryButton>}
                  </div>
                  {playingSample === voice.id && sampleError && <p role="alert" className="mt-1">{labels.sampleError}</p>}
                  {installing === voice.id && <WorkProgress label={activeStage ?? labels.installing} percent={percent} />}
                </li>
              );
            })}
          </ul>
          {visible.length < matches.length && (
            <SecondaryButton className="mt-2" onClick={() => setVisibleCount(count => count + PAGE_SIZE)}>{labels.showMore}</SecondaryButton>
          )}
        </>
      )}
    </div>
  );
}


function SourceDemo({ url, name, onError }: { url: string; name: string; onError: () => void }) {
  const player = useRef<HTMLAudioElement>(null);
  useEffect(() => {
    const audio = player.current;
    return () => audio?.pause();
  }, []);
  return <audio ref={player} src={url} controls autoPlay aria-label={name} onError={onError} className="max-w-full" />;
}
