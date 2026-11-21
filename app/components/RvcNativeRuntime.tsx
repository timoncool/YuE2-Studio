import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { rvcApplyStrings } from '../i18n/rvcApply';
import { poll } from '../services/poll';
import { responseJson } from '../services/rvcVoices';

interface NativeRuntime {
  supported: boolean;
  installed: boolean;
  missing_bytes: number;
  reason: string | null;
  download: { downloaded_bytes: number; total_bytes: number; done: boolean; error?: string | null } | null;
}

export function RvcNativeRuntime({ onInstalled, disabled, metal = false }: {
  onInstalled: () => void;
  disabled: boolean;
  metal?: boolean;
}) {
  const url = metal ? '/v1/rvc/native/runtime?metal=true' : '/v1/rvc/native/runtime';
  const { language, t } = useI18n();
  const labels = rvcApplyStrings[language];
  const [status, setStatus] = useState<NativeRuntime | null>(null);
  const [pending, setPending] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string>();
  const [statusError, setStatusError] = useState<string>();
  const [retry, setRetry] = useState(0);
  const reportedInstalled = useRef(false);

  useEffect(() => {
    const controller = new AbortController();
    async function refresh() {
      try {
        const next = await responseJson<NativeRuntime | null>(await fetch(url, { signal: controller.signal }));
        if (!next || controller.signal.aborted) return;
        setStatusError(undefined);
        setStatus(next);
        if (next.installed && !reportedInstalled.current) {
          reportedInstalled.current = true;
          onInstalled();
        }
        if (pending) {
          if (next.installed || next.download?.done || next.download?.error) setPending(false);
        } else if (next.download && !next.download.done && !next.download.error) {
          setPending(true);
        }
      } catch (failure) {
        if (!controller.signal.aborted) setStatusError(failure instanceof Error ? failure.message : String(failure));
      }
    }
    const stop = pending ? poll(refresh, 500) : undefined;
    if (!pending) void refresh();
    return () => { controller.abort(); stop?.(); };
  }, [pending, onInstalled, retry, url]);

  async function install() {
    setError(undefined);
    setStarting(true);
    try {
      await responseJson(await fetch(url, { method: 'POST' }));
      setPending(true);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setStarting(false);
    }
  }

  async function cancel() {
    try {
      setStatus(await responseJson<NativeRuntime>(await fetch(url, { method: 'DELETE' })));
      setPending(false);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  }

  if (statusError && !pending) {
    return <div className="space-y-1">
      <p role="alert" className="text-rose-600 dark:text-rose-300">{statusError}</p>
      <button type="button" className="font-semibold text-pink-600 underline dark:text-pink-300"
        onClick={() => { setStatusError(undefined); setRetry(value => value + 1); }}>{labels.retry}</button>
    </div>;
  }
  if (!status?.supported || status.installed) return null;
  const bytes = new Intl.NumberFormat(language, { style: 'unit', unit: 'megabyte', maximumFractionDigits: 0 })
    .format(status.missing_bytes / 1_000_000);
  const download = status.download;
  return (
    <div className="space-y-1">
      {pending ? (
        <>
          <p role="status">{labels.installing}</p>
          {download && download.total_bytes > 0 && (
            <progress className="w-full accent-pink-500" aria-label={labels.installing}
              value={Math.min(download.downloaded_bytes, download.total_bytes)} max={download.total_bytes} />
          )}
          <button type="button" className="font-semibold text-pink-600 underline dark:text-pink-300"
            onClick={() => void cancel()}>{t('cancel')}</button>
        </>
      ) : (
        <button type="button" disabled={disabled || starting} className="font-semibold text-pink-600 underline disabled:opacity-50 dark:text-pink-300"
          onClick={() => void install()}>{labels.installNative} · {bytes}</button>
      )}
      {(error || statusError || download?.error) && <p role="alert" className="text-rose-600 dark:text-rose-300">{error || statusError || download?.error}</p>}
    </div>
  );
}
