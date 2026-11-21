import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { rvcApplyStrings } from '../i18n/rvcApply';
import { poll } from '../services/poll';
import { responseJson } from '../services/rvcVoices';

const URL = '/v1/rvc/native/runtime';
interface NativeRuntime {
  supported: boolean;
  installed: boolean;
  missing_bytes: number;
  reason: string | null;
  download: { downloaded_bytes: number; total_bytes: number; done: boolean; error?: string | null } | null;
}

export function RvcNativeRuntime({ onInstalled, disabled }: { onInstalled: () => void; disabled: boolean }) {
  const { language, t } = useI18n();
  const labels = rvcApplyStrings[language];
  const [status, setStatus] = useState<NativeRuntime | null>(null);
  const [pending, setPending] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string>();
  const reportedInstalled = useRef(false);

  useEffect(() => {
    const controller = new AbortController();
    async function refresh() {
      try {
        const next = await responseJson<NativeRuntime | null>(await fetch(URL, { signal: controller.signal }));
        if (!next || controller.signal.aborted) return;
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
      } catch {
        // The explicit install or cancel operation still reports request errors.
      }
    }
    const stop = pending ? poll(refresh, 500) : undefined;
    if (!pending) void refresh();
    return () => { controller.abort(); stop?.(); };
  }, [pending, onInstalled]);

  async function install() {
    setError(undefined);
    setStarting(true);
    try {
      await responseJson(await fetch(URL, { method: 'POST' }));
      setPending(true);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setStarting(false);
    }
  }

  async function cancel() {
    try {
      setStatus(await responseJson<NativeRuntime>(await fetch(URL, { method: 'DELETE' })));
      setPending(false);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
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
          onClick={() => void install()}>{labels.installNative} В· {bytes}</button>
      )}
      {(error || download?.error) && <p role="alert" className="text-rose-600 dark:text-rose-300">{error || download?.error}</p>}
    </div>
  );
}
