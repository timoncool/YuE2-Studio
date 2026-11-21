import { useEffect, useState } from 'react';
import { ArrowRight } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { signedUpdateStrings } from '../i18n/signedUpdates';
import { SELF_READ } from '../services/studioQueries';

const modes = ['manual', 'notify', 'auto'] as const;
type Mode = typeof modes[number];
type PackageKind = 'music-engines' | 'music-models';

interface PackageUpdate {
  kind: PackageKind;
  id: string;
  from_version: string;
  to_version: string;
  changed_bytes: number;
}

interface SignedUpdates {
  mode: Mode;
  updates: PackageUpdate[];
}

const statusUrl = '/v1/components/updates';
const json = { 'Content-Type': 'application/json' };

async function responseBody(response: Response) {
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error ?? body?.message ?? `HTTP ${response.status}`);
  return body;
}

async function readUpdates(signal?: AbortSignal): Promise<SignedUpdates | null> {
  const response = await fetch(statusUrl, { headers: SELF_READ, signal });
  if (response.status === 404) return null;
  const body = await responseBody(response);
  if (!modes.includes(body?.mode) || !Array.isArray(body?.updates)) return null;
  return body;
}

function formatBytes(value: number, language: string): string {
  const [divisor, unit] = value >= 2 ** 30 ? [2 ** 30, 'GiB']
    : value >= 2 ** 20 ? [2 ** 20, 'MiB'] : value >= 2 ** 10 ? [2 ** 10, 'KiB'] : [1, 'B'];
  return `${new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(value / divisor)} ${unit}`;
}

export function SignedComponentUpdates() {
  const { language } = useI18n();
  const text = signedUpdateStrings[language];
  const [status, setStatus] = useState<SignedUpdates | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  useEffect(() => {
    const controller = new AbortController();
    void readUpdates(controller.signal).then(next => {
      if (!controller.signal.aborted) setStatus(next);
    }, (problem: unknown) => {
      if (!controller.signal.aborted) setError(problem instanceof Error ? problem.message : String(problem));
    });
    return () => controller.abort();
  }, []);

  const act = async (url: string, mode?: Mode) => {
    setBusy(true);
    setError(undefined);
    try {
      await fetch(url, { method: 'POST', ...(mode ? { headers: json, body: JSON.stringify({ mode }) } : {}) }).then(responseBody);
      setStatus(await readUpdates());
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  if (!status && !error) return null;
  const name = (kind: PackageKind) => kind === 'music-engines' ? text.engine : text.model;
  const control = 'rounded-lg border border-zinc-200 px-2 py-1 text-sm disabled:opacity-50 dark:border-white/15';
  return (
    <section aria-label={text.title} aria-busy={busy} className="space-y-3 rounded-xl border border-zinc-200 bg-white px-4 py-3 dark:border-white/10 dark:bg-suno-card">
      <h3 className="text-sm font-semibold">{text.title}</h3>
      <div className="flex flex-wrap items-center gap-2">
        {status && (
          <label className="flex items-center gap-2 text-xs">
            {text.mode}
            <select className={control} value={status.mode} disabled={busy} onChange={event => {
              const mode = modes.find(mode => mode === event.target.value);
              if (mode) void act(`${statusUrl}/mode`, mode);
            }}>
              {modes.map(mode => <option key={mode} value={mode}>{text[mode]}</option>)}
            </select>
          </label>
        )}
        <button type="button" className={control} disabled={busy}
          onClick={() => void act('/v1/components/music-engines/refresh')}>{text.refresh}</button>
      </div>
      {error && <p role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</p>}
      {status && !status.updates.length && <p className="text-xs text-zinc-500 dark:text-zinc-400">{text.empty}</p>}
      {status && status.updates.length > 0 && (
        <ul className="divide-y divide-zinc-100 dark:divide-white/10">
          {status.updates.map(update => (
            <li key={`${update.kind}/${update.id}`} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2">
              <div className="mr-auto min-w-0 text-sm">
                <span className="break-all font-semibold">{update.id}</span>
                <span className="ml-2 text-xs text-zinc-500 dark:text-zinc-400">{name(update.kind)}</span>
              </div>
              <span className="flex items-center gap-1 whitespace-nowrap font-mono text-xs text-zinc-500 dark:text-zinc-400">
                {update.from_version}<ArrowRight size={12} aria-hidden="true" />{update.to_version}
              </span>
              <span className="whitespace-nowrap text-xs tabular-nums text-zinc-500 dark:text-zinc-400">{formatBytes(update.changed_bytes, language)}</span>
              <button type="button" className={control} disabled={busy}
                aria-label={`${text.update} ${update.id}`}
                onClick={() => void act(`/v1/components/${update.kind}/${encodeURIComponent(update.id)}/install`)}>{text.update}</button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
