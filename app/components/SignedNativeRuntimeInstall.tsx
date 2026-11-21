import { useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useI18n } from '../context/I18nContext';
import { musicSourceStrings } from '../i18n/musicSource';
import { apiUrl } from '../services/apiBase';
import { postSignedPackage, signedPackageBytes, type EnginePackage, type PackageProgress } from '../services/signedMusicSelection';

export function SignedNativeRuntimeInstall({ pack, onInstalled }: { pack: EnginePackage; onInstalled: () => Promise<void> }) {
  const { t, language } = useI18n();
  const strings = musicSourceStrings[language];
  const id = encodeURIComponent(pack.id);
  const path = `/v1/components/packages/music-engines/${id}`;
  const operation = useRef<{ cancelled: boolean } | undefined>(undefined);
  const [installing, setInstalling] = useState(false);
  const [cancelPending, setCancelPending] = useState(false);
  const [error, setError] = useState('');
  const { data: progress } = useQuery({
    queryKey: [path, 'progress'],
    queryFn: async ({ signal }) => {
      const response = await fetch(apiUrl(`${path}/progress`), { signal });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const body: { active: PackageProgress | null } = await response.json();
      return body.active;
    },
    enabled: installing,
    refetchInterval: installing ? 400 : false,
  });

  async function install() {
    if (operation.current) return;
    const run = { cancelled: false };
    operation.current = run;
    setError('');
    setInstalling(true);
    try {
      await postSignedPackage(`/v1/components/music-engines/${id}/install`);
      await onInstalled();
    } catch (problem) {
      const message = problem instanceof Error ? problem.message : String(problem);
      if (!message.includes('Package installation cancelled')) setError(message);
    } finally {
      operation.current = undefined;
      setInstalling(false);
      setCancelPending(false);
    }
  }

  async function cancel() {
    const run = operation.current;
    if (!run || cancelPending) return;
    setCancelPending(true);
    try {
      const response = await postSignedPackage(`${path}/cancel`);
      const result: { cancel_requested?: boolean } = await response.json();
      run.cancelled = result.cancel_requested === true;
      if (operation.current === run && !run.cancelled) setCancelPending(false);
    } catch (problem) {
      if (operation.current === run) {
        setError(problem instanceof Error ? problem.message : String(problem));
        setCancelPending(false);
      }
    }
  }

  const percent = progress?.total_bytes
    ? Math.min(100, Math.floor(100 * (progress.downloaded_bytes + progress.reused_bytes) / progress.total_bytes))
    : 0;
  const size = new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(signedPackageBytes(pack.files) / 2 ** 30);
  return <div className="space-y-1 text-xs">
    <button type="button" disabled={installing} onClick={() => void install()}
      className="text-pink-600 underline disabled:opacity-50 dark:text-pink-300">
      {strings.installModel} · {pack.version} · {size} GiB
    </button>
    {installing && <>
      <p role="status">{progress ? `${strings.transferStages[progress.stage]} ${percent}%` : strings.installing}</p>
      {progress && progress.stage !== 'activating' && <button type="button" disabled={cancelPending}
        className="underline" onClick={() => void cancel()}>{t('cancelDownload')}</button>}
    </>}
    {error && <p role="alert">{error}</p>}
  </div>;
}
