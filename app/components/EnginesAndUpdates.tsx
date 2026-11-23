import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { componentCatalogueStrings, engineCardStrings, enginePageStrings, runtimeSourceStrings, showUnsupportedEngines } from '../i18n/enginePage';
import { apiUrl } from '../services/apiBase';
import type { PairStatus } from '../services/musicPair';
import { SELF_READ, useOpenRouterSettings, useSetupStatus } from '../services/studioQueries';
import { NativeMusicRuntimeInstall } from './NativeMusicRuntimeInstall';
import { AudioCppModelInstall } from './HeartMuLaControls';
import { UpdatePlanView } from './UpdateScreen';
import { RvcNativeRuntime } from './RvcNativeRuntime';
import { CatalogBadge } from './kit/Catalog';
import { SecondaryButton } from './kit/Buttons';

const native = [
  { id: 'acestep-cpp', name: 'ACE-Step', path: '/v1/music/native/acestep-cpp/profiles' },
  { id: 'minimaxmusic-cpp', name: 'MiniMax Music3', path: '/v1/music/native/minimaxmusic-cpp/profiles' },
  { id: 'audio.cpp', name: 'audio.cpp', path: '/v1/music/audio-cpp/models' },
  { id: 'hotstep', name: 'HOT-Step', path: '/v1/music/native/hotstep/profiles' },
] as const;
type ModelStatus = { id: string; label?: string; name?: string; ready: boolean; missing_bytes?: number; install_path?: string | null };
type ProfileStatus = {
  engine_installed: boolean;
  adapter_runtime_update?: boolean;
  selected_executable?: { path: string; source: 'local' | 'managed' } | null;
  profiles?: ModelStatus[];
  models?: ModelStatus[];
};
type RuntimeStatus = { supported: boolean; installed?: boolean; revision?: string; installed_revision?: string | null };
type RvcStatus = { installed: boolean; supported: boolean; reason?: string | null };
type ComponentsState = { sequence: number; published: string; source: 'built-in' | 'downloaded'; checked_at?: number | null; refresh_error?: string | null };
function modelInstallPath(engine: string, path?: string | null) {
  if (engine === 'audio.cpp' && path === '/v1/music/audio-cpp/models/install') return path;
  if (engine === 'hotstep' && path && /^\/v1\/music\/native\/hotstep\/(?:[a-z0-9-]+\/)?install$/.test(path)) return path;
  if ((engine === 'acestep-cpp' || engine === 'minimaxmusic-cpp') && path === `/v1/music/native/${engine}/profiles/install`) return path;
}

async function read<T>(path: string, signal: AbortSignal): Promise<T | null> {
  try {
    const response = await fetch(apiUrl(path), { signal, headers: SELF_READ });
    return response.ok ? await response.json() : null;
  } catch {
    return null;
  }
}

/** One engine: its name and state at a glance, what it takes next, and the technical facts folded away. */
function EngineCard({ id, name, focused, badges, details, detailsLabel, children }: {
  id: string;
  name: string;
  focused: boolean;
  badges: ReactNode;
  details?: ReactNode;
  detailsLabel: string;
  children?: ReactNode;
}) {
  return (
    <li data-engine={id} aria-label={name} className={`space-y-2 rounded-2xl border p-4 ${focused ? 'border-pink-500' : 'border-zinc-200 dark:border-white/10'}`}>
      <div className="flex flex-wrap items-center gap-2">
        <h4 className="mr-1 text-sm font-semibold text-zinc-900 dark:text-white">{name}</h4>
        {badges}
      </div>
      {children}
      {details && (
        <details className="text-xs text-zinc-500 dark:text-zinc-400">
          <summary className="cursor-pointer select-none hover:text-pink-600">{detailsLabel}</summary>
          <div className="mt-1.5 space-y-1 break-all">{details}</div>
        </details>
      )}
    </li>
  );
}

export function EnginesAndUpdates({ focus, onNavigate }: {
  focus?: string;
  onNavigate: (target: 'models' | 'engine' | 'cloud') => void;
}) {
  const { language, t } = useI18n();
  const [showUnsupported, setShowUnsupported] = useState(false);
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    const target = [...(list.current?.children ?? [])].find(item => item.getAttribute('data-engine') === focus);
    target?.scrollIntoView?.({ block: 'nearest' });
  }, [focus]);
  const [title, active, installed, missing, checking, unavailable, manageModels, settings] = enginePageStrings[language];
  const [managedPackage, localExecutable] = runtimeSourceStrings[language];
  const catalogueText = componentCatalogueStrings[language];
  const card = engineCardStrings[language];
  const setup = useSetupStatus<{ ready?: boolean; engine_ready?: boolean }>();
  const cloud = useOpenRouterSettings<{ configured?: boolean }>();
  const catalogue = useQuery({
    queryKey: ['engine-page'],
    queryFn: async ({ signal }) => {
      const [pair, local, rvc, profiles, ace, minimax, components] = await Promise.all([
        read<PairStatus>('/v1/music/pair', signal),
        read<{ executable?: string | null; executable_source?: string; in_effect?: { backend?: string } }>('/engine/options', signal),
        read<RvcStatus>('/v1/rvc/native/runtime', signal),
        Promise.all(native.map(engine => read<ProfileStatus>(engine.path, signal))),
        read<RuntimeStatus>('/v1/music/native/acestep-cpp/runtime', signal),
        read<RuntimeStatus>('/v1/music/native/minimaxmusic-cpp/runtime', signal),
        read<ComponentsState>('/v1/components', signal),
      ]);
      return { pair, local, rvc, profiles, ace, minimax, components };
    },
    staleTime: 0,
  });
  const data = catalogue.data;
  const [refreshingCatalogue, setRefreshingCatalogue] = useState(false);
  const [catalogueError, setCatalogueError] = useState('');
  async function refreshCatalogue() {
    setRefreshingCatalogue(true);
    setCatalogueError('');
    try {
      const components = await fetch(apiUrl('/v1/components/refresh'), { method: 'POST', headers: SELF_READ });
      if (!components.ok) {
        const body = await components.json().catch(() => null) as { error?: string } | null;
        throw new Error(body?.error ?? `HTTP ${components.status}`);
      }
    } catch (error) {
      setCatalogueError(error instanceof Error ? error.message : String(error));
    } finally {
      setRefreshingCatalogue(false);
      void catalogue.refetch();
    }
  }
  const hasUnsupported = data?.rvc?.supported === false
    || (data?.ace?.supported === false && data.profiles[0]?.engine_installed === false)
    || (data?.minimax?.supported === false && data.profiles[1]?.engine_installed === false);
  const refresh = async () => { await Promise.all([catalogue.refetch(), setup.refetch(), cloud.refetch()]); };
  const busy = catalogue.isFetching || setup.isFetching || cloud.isFetching || refreshingCatalogue;
  const installState = (value: boolean | undefined) => value === undefined
    ? <CatalogBadge tone="muted">{catalogue.isPending ? checking : unavailable}</CatalogBadge>
    : <CatalogBadge tone={value ? 'good' : 'muted'}>{value ? installed : missing}</CatalogBadge>;
  const modelsState = (ready: boolean | undefined) => ready === undefined ? null
    : <CatalogBadge tone={ready ? 'good' : 'muted'}>{ready ? card.modelsReady : card.modelsMissing}</CatalogBadge>;
  const activeBadge = <CatalogBadge>{active}</CatalogBadge>;
  const activePair = data?.pair?.active;
  const activeId = activePair?.kind === 'native' ? activePair.engine : activePair?.kind === 'audio_cpp' ? 'audio.cpp' : activePair?.kind;
  const catalogueProblem = catalogueError || data?.components?.refresh_error;
  return (
    <section aria-label={title} className="space-y-4">
      {focus === 'updates' && <UpdatePlanView />}
      <div className="flex flex-wrap items-center gap-3">
        <SecondaryButton disabled={busy} onClick={() => {
          if (!refreshingCatalogue) void refreshCatalogue();
          void refresh();
        }}><RefreshCw size={14} className={`mr-1.5 inline ${busy ? 'animate-spin' : ''}`} aria-hidden />{t('refresh')}</SecondaryButton>
        {hasUnsupported && <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={showUnsupported} onChange={event => setShowUnsupported(event.target.checked)} />
          {showUnsupportedEngines[language]}
        </label>}
      </div>
      {data?.components && <p className="text-xs text-zinc-500 dark:text-zinc-400">
        {catalogueText.list(data.components.sequence, new Date(data.components.published).toLocaleDateString(language))}
        {data.components.source === 'built-in' && <> · {catalogueText.builtIn}</>}
        {data.components.checked_at && <> · {catalogueText.checked(new Date(data.components.checked_at * 1000).toLocaleString(language))}</>}
      </p>}
      {catalogueProblem && (
        <div role={catalogueError ? 'alert' : 'status'} className="flex flex-wrap items-center gap-2 rounded-xl border border-amber-500/25 bg-amber-500/10 p-3 text-xs leading-5 text-amber-800 dark:text-amber-200">
          <span className="min-w-0 flex-1" title={catalogueProblem}>{card.offline}</span>
          <SecondaryButton disabled={refreshingCatalogue} onClick={() => void refreshCatalogue()}>{card.retry}</SecondaryButton>
        </div>
      )}
      <ul ref={list} className="grid gap-2" aria-label={title}>
        <EngineCard id="local" name="YuE2" focused={focus === 'local'} detailsLabel={card.details}
          badges={<>{installState(data?.local ? Boolean(data.local.executable || setup.data?.engine_ready) : undefined)}
            {activeId === 'local' && activeBadge}{modelsState(setup.data?.ready)}</>}
          details={data?.local?.executable ? <>
            <p>{data.local.executable}{data.local.executable_source === 'environment' && <> · {t('engineBinEnvOverride')}</>}</p>
            {data.local.in_effect?.backend && <p>{data.local.in_effect.backend}</p>}
          </> : undefined}>
          <div className="flex flex-wrap gap-2">
            <SecondaryButton onClick={() => onNavigate('models')}>{manageModels}</SecondaryButton>
            <SecondaryButton onClick={() => onNavigate('engine')}>{settings}</SecondaryButton>
          </div>
        </EngineCard>
        {native.map((engine, index) => {
          const profile = data?.profiles[index];
          const models = profile?.profiles ?? profile?.models;
          const ready = models ? models.some(model => model.ready) : undefined;
          const runtime = engine.id === 'acestep-cpp' ? data?.ace : engine.id === 'minimaxmusic-cpp' ? data?.minimax : null;
          const installedRevision = runtime?.installed_revision ?? (runtime?.installed ? runtime.revision : undefined);
          const unsupported = runtime?.supported === false && profile?.engine_installed === false;
          if (unsupported && !showUnsupported && focus !== engine.id) return null;
          const toInstall = (models ?? []).flatMap(model => {
            const installPath = modelInstallPath(engine.id, model.install_path);
            return installPath && !model.ready ? [{ model, installPath, label: model.label ?? model.name ?? model.id }] : [];
          });
          const local = profile?.selected_executable?.source === 'local' ? profile.selected_executable.path : null;
          return <EngineCard key={engine.id} id={engine.id} name={engine.name} focused={focus === engine.id} detailsLabel={card.details}
            badges={<>{unsupported ? <CatalogBadge tone="muted">{card.unsupported}</CatalogBadge> : installState(profile?.engine_installed)}
              {activeId === engine.id && activeBadge}{modelsState(ready)}</>}
            details={local || installedRevision || (runtime?.revision && runtime.revision !== installedRevision) ? <>
              {local && <p>{localExecutable} · {local}</p>}
              {installedRevision && <p>{managedPackage} · {installedRevision.slice(0, 12)}</p>}
              {runtime?.revision && runtime.revision !== installedRevision && <p>{t('download')} · {runtime.revision.slice(0, 12)}</p>}
            </> : undefined}>
            {unsupported && <p className="text-xs text-zinc-500 dark:text-zinc-400">{t('engineUnavailable')}</p>}
            {(engine.id === 'acestep-cpp' || engine.id === 'minimaxmusic-cpp') && profile &&
              runtime?.supported && (!profile.engine_installed || profile.adapter_runtime_update) &&
              <NativeMusicRuntimeInstall engine={engine.id} update={profile.adapter_runtime_update} onInstalled={refresh} />}
            {toInstall.length > 0 && (
              <details open={focus === engine.id} className="text-sm">
                <summary className="cursor-pointer select-none text-xs font-semibold text-zinc-600 hover:text-pink-600 dark:text-zinc-300">{card.models(toInstall.length)}</summary>
                <ul className="mt-2 space-y-2">
                  {toInstall.map(({ model, installPath, label }) => (
                    <li key={model.id} className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
                      <p className="mb-2 text-xs font-medium text-zinc-800 dark:text-zinc-100">{label}</p>
                      <AudioCppModelInstall model={model.id} bytes={model.missing_bytes ?? 0} language={language} onInstalled={refresh}
                        target={{ catalogue: engine.path, installPath, label }} />
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </EngineCard>;
        })}
        {(data?.rvc?.supported !== false || showUnsupported || focus === 'rvc') &&
          <EngineCard id="rvc" name="RVC" focused={focus === 'rvc'} detailsLabel={card.details}
            badges={data?.rvc?.supported === false ? <CatalogBadge tone="muted">{card.unsupported}</CatalogBadge> : installState(data?.rvc?.installed)}>
            {data?.rvc?.supported === false && data.rvc.reason && <p className="text-xs text-zinc-500 dark:text-zinc-400">{data.rvc.reason}</p>}
            {data?.rvc?.supported && !data.rvc.installed && <RvcNativeRuntime onInstalled={refresh} disabled={false} />}
          </EngineCard>}
        <EngineCard id="openrouter" name="OpenRouter" focused={focus === 'openrouter'} detailsLabel={card.details}
          badges={cloud.data
            ? <CatalogBadge tone={cloud.data.configured ? 'good' : 'muted'}>{t(cloud.data.configured ? 'openRouterConfigured' : 'openRouterNotConfigured')}</CatalogBadge>
            : <CatalogBadge tone="muted">{cloud.isPending ? checking : unavailable}</CatalogBadge>}>
          <SecondaryButton onClick={() => onNavigate('cloud')}>{t('providers')}</SecondaryButton>
        </EngineCard>
      </ul>
      {focus !== 'updates' && <UpdatePlanView />}
    </section>
  );
}
