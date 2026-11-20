import { useEffect, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { rvcVoiceStrings } from '../i18n/rvcVoices';
import { responseJson, type Voice } from '../services/rvcVoices';

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
type Filters = Partial<Record<MetadataField | 'index', string>>;

export function RvcCuratedVoices({ onInstalled, onChoose, disabled }: {
  onInstalled: (voice: Voice) => void;
  onChoose: (id: string) => void;
  disabled: boolean;
}) {
  const { language } = useI18n();
  const labels = rvcVoiceStrings[language];
  const [open, setOpen] = useState(false);
  const [catalog, setCatalog] = useState<CatalogVoice[] | null>(null);
  const [request, setRequest] = useState(0);
  const [installing, setInstalling] = useState<string>();
  const [error, setError] = useState<string>();
  const [search, setSearch] = useState('');
  const [filters, setFilters] = useState<Filters>({});
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const sizeFormat = new Intl.NumberFormat(language, { style: 'unit', unit: 'megabyte', maximumFractionDigits: 1 });

  function setFilter(field: MetadataField | 'index', value: string) {
    setFilters(current => ({ ...current, [field]: value }));
    setVisibleCount(PAGE_SIZE);
  }

  useEffect(() => {
    if (!open) return;
    const controller = new AbortController();
    void fetch(URL, { signal: controller.signal }).then(responseJson<CatalogVoice[]>)
      .then(items => { if (!controller.signal.aborted) setCatalog(items); })
      .catch((failure: Error) => { if (!controller.signal.aborted) setError(failure.message); });
    return () => controller.abort();
  }, [open, request]);

  async function install(id: string) {
    setInstalling(id);
    setError(undefined);
    try {
      const voice = await responseJson<Voice>(await fetch(`${URL}/install`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id }),
      }));
      setCatalog(current => current?.map(item => item.id === id ? { ...item, installed_id: voice.id } : item) ?? null);
      onInstalled(voice);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setInstalling(undefined);
    }
  }

  const terms = search.trim().toLocaleLowerCase(language).split(/\s+/).filter(Boolean);
  const matches = (catalog ?? []).filter(voice => {
    const searchable = [voice.name, ...(voice.metadata?.source_labels ?? [])].join(' ').toLocaleLowerCase(language);
    return terms.every(term => searchable.includes(term))
      && metadataFields.every(field => !filters[field] || String(voice.metadata?.[field] ?? '') === filters[field])
      && (!filters.index || ((voice.has_index ?? !!voice.index) ? 'present' : 'absent') === filters.index);
  });
  const visible = matches.slice(0, visibleCount);
  const filterActive = terms.length > 0 || Object.values(filters).some(Boolean);
  const filterLabels = {
    vocal_style: labels.vocalStyle,
    rvc_version: labels.rvcVersion,
    sample_rate_hz: labels.sampleRate,
  };

  return (
    <details className="mt-4 border-t border-zinc-200 pt-3 text-xs dark:border-white/10"
      onToggle={event => setOpen(event.currentTarget.open)}>
      <summary className="cursor-pointer text-sm font-semibold">{labels.curated}</summary>
      {open && (
        <div className="mt-2">
          {!catalog && !error && <p role="status">{labels.loading}</p>}
          {catalog?.length === 0 && <p>{labels.empty}</p>}
          {error && <p role="alert" className="break-words text-rose-600 dark:text-rose-300">{error}</p>}
          {!catalog && error && (
            <button type="button" className="text-pink-600 underline dark:text-pink-300"
              onClick={() => { setError(undefined); setRequest(value => value + 1); }}>{labels.retry}</button>
          )}
          {catalog && catalog.length > 0 && (
            <>
              <div className="mb-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                <label className="block space-y-1 sm:col-span-2 lg:col-span-4">
                  <span>{labels.search}</span>
                  <input type="search" value={search} onChange={event => { setSearch(event.target.value); setVisibleCount(PAGE_SIZE); }}
                    className="w-full rounded border border-zinc-200 px-2 py-1 dark:border-white/10" />
                </label>
                {metadataFields.map(field => {
                  const options = [...new Set(catalog.flatMap(voice => {
                    const value = voice.metadata?.[field];
                    return value == null || value === '' ? [] : [String(value)];
                  }))].sort((a, b) => a.localeCompare(b, language, { numeric: true }));
                  return (
                    <label key={field} className="block space-y-1">
                      <span>{filterLabels[field]}</span>
                      <select value={filters[field] ?? ''} disabled={options.length === 0}
                        className="w-full rounded border border-zinc-200 px-2 py-1 dark:border-white/10"
                        onChange={event => setFilter(field, event.target.value)}>
                        <option value="">{labels.all}</option>
                        {options.map(value => <option key={value} value={value}>{field === 'sample_rate_hz' ? `${value} Hz` : value}</option>)}
                      </select>
                    </label>
                  );
                })}
                <label className="block space-y-1">
                  <span>{labels.retrieval}</span>
                  <select value={filters.index ?? ''} className="w-full rounded border border-zinc-200 px-2 py-1 dark:border-white/10"
                    onChange={event => setFilter('index', event.target.value)}>
                    <option value="">{labels.all}</option>
                    <option value="present">{labels.withIndex}</option>
                    <option value="absent">{labels.withoutIndex}</option>
                  </select>
                </label>
              </div>
              {filterActive && (
                <button type="button" className="mb-2 text-pink-600 underline dark:text-pink-300"
                  onClick={() => { setSearch(''); setFilters({}); setVisibleCount(PAGE_SIZE); }}>{labels.clear}</button>
              )}
              <p role="status" className="mb-1 text-zinc-600 dark:text-zinc-300">
                {matches.length === 0 ? labels.noMatch : labels.resultCount
                  .replace('{visible}', visible.length.toLocaleString(language))
                  .replace('{total}', matches.length.toLocaleString(language))}
              </p>
              <ul className="max-h-80 divide-y divide-zinc-200 overflow-auto dark:divide-white/10">
              {visible.map(voice => {
                const size = (voice.download_bytes ?? ((voice.weights?.size ?? 0) + (voice.index?.size ?? 0))) / 1_000_000;
                return (
                  <li key={voice.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
                    <div className="min-w-0">
                      <p className="font-semibold">{voice.name}</p>
                      <p className="mt-1 text-zinc-600 dark:text-zinc-300">
                        <a href={voice.source_url} target="_blank" rel="noreferrer" className="underline">{voice.source_repo}</a>
                        {' · '}{voice.license ?? labels.licenseUnknown}{' · '}
                        {sizeFormat.format(size)}
                      </p>
                    </div>
                    <button type="button" disabled={disabled || Boolean(installing)}
                      aria-label={`${voice.installed_id ? labels.selectInstalled : labels.install} ${voice.name}`}
                      onClick={() => voice.installed_id ? onChoose(voice.installed_id) : void install(voice.id)}
                      className="rounded-lg border border-zinc-200 px-3 py-2 enabled:hover:border-pink-500 disabled:opacity-50 dark:border-white/10">
                      {voice.installed_id ? labels.selectInstalled : installing === voice.id ? labels.installing : labels.install}
                    </button>
                  </li>
                );
              })}
              </ul>
              {visible.length < matches.length && (
                <button type="button" className="mt-2 text-pink-600 underline dark:text-pink-300"
                  onClick={() => setVisibleCount(count => count + PAGE_SIZE)}>{labels.showMore}</button>
              )}
            </>
          )}
        </div>
      )}
    </details>
  );
}
