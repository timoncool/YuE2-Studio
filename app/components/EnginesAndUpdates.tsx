import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { componentCatalogueStrings, engineCardStrings, enginePageStrings, runtimeSourceStrings, showUnsupportedEngines } from '../i18n/enginePage';
import { apiUrl } from '../services/apiBase';
import type { PairStatus } from '../services/musicPair';
import { SELF_READ, useSetupStatus } from '../services/studioQueries';
import { engineCardText } from '../i18n/engineCards';
import { musicSourceStrings, nativeMusicSourceStrings } from '../i18n/musicSource';
import { useNativeMusicProfiles } from '../services/nativeMusicProfiles';
import { NativeModelSetPicker } from './NativeModelSetPicker';
import { YueModelSetPicker } from './YueModelSetPicker';
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
type ModelStatus = { id: string; label?: string; name?: string; ready: boolean; missing_bytes?: number; install_path?: string | null; status?: string };
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
function EngineCard({ id, name, about, focused, status, inUse, details, detailsLabel, children }: {
  id: string;
  name: string;
  about?: string;
  focused: boolean;
  status: ReactNode;
  inUse?: ReactNode;
  details?: ReactNode;
  detailsLabel: string;
  children?: ReactNode;
}) {
  return (
    <li data-engine={id} aria-label={name} className={`space-y-3 rounded-2xl border p-4 ${focused ? 'border-pink-500' : 'border-zinc-200 dark:border-white/10'}`}>
      <div>
        <div className="flex flex-wrap items-center gap-2">
          <h4 className="mr-1 text-sm font-semibold text-zinc-900 dark:text-white">{name}</h4>
          {status}
          {inUse}
        </div>
        {about && <p className="mt-1 text-xs leading-5 text-zinc-500 dark:text-zinc-400">{about}</p>}
      </div>
      {children}
      {details && (
        <details className="text-xs text-zinc-500 dark:text-zinc-400">
          <summary className="min-h-6 cursor-pointer select-none hover:text-pink-600">{detailsLabel}</summary>
          <div className="mt-1.5 space-y-1 break-all">{details}</div>
        </details>
      )}
    </li>
  );
}


/** ACE-Step and MiniMax: the same quality choice as in the create form, so the words, sizes and downloads match. */
function NativeEngineQuality({ engine, name, onChanged }: { engine: 'acestep-cpp' | 'minimaxmusic-cpp'; name: string; onChanged: () => void }) {
  const native = useNativeMusicProfiles(engine);
  if (!native.profiles.length) return null;
  return <NativeModelSetPicker engine={engine} name={name} profiles={native.profiles} selected={native.profile} engineInstalled={native.installed}
    onSelect={native.select} onChanged={() => { native.refresh(); onChanged(); }} />;
}

/** audio.cpp and HOT-Step: every model as a row with what it does, its state and its own download. */
function ModelRows({ engine, catalogue, models, onChanged }: { engine: string; catalogue: string; models: ModelStatus[]; onChanged: () => void }) {
  const { language } = useI18n();
  const text = engineCardText[language];
  const hints = musicSourceStrings[language].hintModels as Record<string, string>;
  return (
    <div className="space-y-1.5">
      <p className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{text.models}</p>
      <p className="text-xs text-zinc-500 dark:text-zinc-400">{text.modelsHint}</p>
      <ul className="divide-y divide-zinc-100 rounded-xl border border-zinc-200 dark:divide-white/5 dark:border-white/10">
        {models.filter(model => model.ready || modelInstallPath(engine, model.install_path)).map(model => {
          const [title, ...rest] = (model.label ?? model.name ?? model.id).split(' · ');
          const installPath = modelInstallPath(engine, model.install_path);
          const quant = rest.filter(part => part !== 'HOT-Step').join(' · ').replace(/\s*\(experimental\)/i, '');
          const meta = [hints[model.id], quant, model.status === 'experimental' ? nativeMusicSourceStrings[language].experimental : ''].filter(Boolean).join(' · ');
          return (
            <li key={model.id} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium text-zinc-900 dark:text-white">{title}</span>
                {meta && <span className="block text-xs text-zinc-500 dark:text-zinc-400">{meta}</span>}
              </span>
              {model.ready ? <span className="text-xs font-semibold text-emerald-700 dark:text-emerald-300">{text.downloaded}</span>
                : installPath && <AudioCppModelInstall secondary compact model={model.id} bytes={model.missing_bytes ?? 0} language={language} onInstalled={onChanged}
                  target={{ catalogue, installPath, label: title }} />}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export function EnginesAndUpdates({ focus }: {
  focus?: string;
  onNavigate?: (target: 'models' | 'engine' | 'cloud') => void;
}) {
  const { language, t } = useI18n();
  const [showUnsupported, setShowUnsupported] = useState(false);
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    const target = [...(list.current?.children ?? [])].find(item => item.getAttribute('data-engine') === focus);
    target?.scrollIntoView?.({ block: 'nearest' });
  }, [focus]);
  const [title, , , , checking, unavailable] = enginePageStrings[language];
  const [managedPackage, localExecutable] = runtimeSourceStrings[language];
  const catalogueText = componentCatalogueStrings[language];
  const card = engineCardStrings[language];
  const text = engineCardText[language];
  const setup = useSetupStatus<{ ready?: boolean; engine_ready?: boolean }>();
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
  const refresh = async () => { await Promise.all([catalogue.refetch(), setup.refetch()]); };
  const busy = catalogue.isFetching || setup.isFetching || refreshingCatalogue;
  // one state in words: what the engine can do now, not three badges to add up
  const state = (installed: boolean | undefined, ready: boolean | undefined, unsupported = false) => unsupported
    ? <CatalogBadge tone="muted">{card.unsupported}</CatalogBadge>
    : installed === undefined ? <CatalogBadge tone="muted">{catalogue.isPending ? checking : unavailable}</CatalogBadge>
    : !installed ? <CatalogBadge tone="muted">{text.notInstalled}</CatalogBadge>
    : ready === false ? <CatalogBadge tone="muted">{text.noModel}</CatalogBadge>
    : <CatalogBadge tone="good">{text.ready}</CatalogBadge>;
  const inUse = <CatalogBadge>{text.inUse}</CatalogBadge>;
  const activePair = data?.pair?.active;
  const activeId = activePair?.kind === 'native' ? activePair.engine : activePair?.kind === 'audio_cpp' ? 'audio.cpp' : activePair?.kind;
  const catalogueProblem = catalogueError || data?.components?.refresh_error;
  return (
    <section aria-label={title} className="space-y-4">
      <UpdatePlanView quietWhenEmpty />
      <div className="flex flex-wrap items-center gap-3">
        <SecondaryButton disabled={busy} onClick={() => {
          if (!refreshingCatalogue) void refreshCatalogue();
          void refresh();
        }}><RefreshCw size={14} className={`mr-1.5 inline ${busy ? 'animate-spin' : ''}`} aria-hidden />{text.checkUpdates}</SecondaryButton>
        {hasUnsupported && <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={showUnsupported} onChange={event => setShowUnsupported(event.target.checked)} />
          {showUnsupportedEngines[language]}
        </label>}
      </div>
      {catalogueProblem && (
        <div role={catalogueError ? 'alert' : 'status'} className="flex flex-wrap items-center gap-2 rounded-xl border border-amber-500/25 bg-amber-500/10 p-3 text-xs leading-5 text-amber-800 dark:text-amber-200">
          <span className="min-w-0 flex-1" title={catalogueProblem}>{card.offline}</span>
          <SecondaryButton disabled={refreshingCatalogue} onClick={() => void refreshCatalogue()}>{card.retry}</SecondaryButton>
        </div>
      )}
      <ul ref={list} className="grid gap-3" aria-label={title}>
        <EngineCard id="local" name="YuE2" about={text.about.local} focused={focus === 'local'} detailsLabel={text.versionAndFiles}
          status={state(data?.local ? Boolean(data.local.executable || setup.data?.engine_ready) : undefined, setup.data?.ready)}
          inUse={activeId === 'local' ? inUse : undefined}
          details={data?.local?.executable ? <>
            <p>{localExecutable} · {data.local.executable}{data.local.executable_source === 'environment' && <> · {t('engineBinEnvOverride')}</>}</p>
            {data.local.in_effect?.backend && <p>{data.local.in_effect.backend}</p>}
          </> : undefined}>
          <YueModelSetPicker onChanged={() => void refresh()} />
        </EngineCard>
        {native.map((engine, index) => {
          const profile = data?.profiles[index];
          const models = profile?.profiles ?? profile?.models;
          const ready = models ? models.some(model => model.ready) : undefined;
          const runtime = engine.id === 'acestep-cpp' ? data?.ace : engine.id === 'minimaxmusic-cpp' ? data?.minimax : null;
          const installedRevision = runtime?.installed_revision ?? (runtime?.installed ? runtime.revision : undefined);
          const unsupported = runtime?.supported === false && profile?.engine_installed === false;
          if (unsupported && !showUnsupported && focus !== engine.id) return null;
          const local = profile?.selected_executable?.source === 'local' ? profile.selected_executable.path : null;
          const sets = engine.id === 'acestep-cpp' || engine.id === 'minimaxmusic-cpp';
          return <EngineCard key={engine.id} id={engine.id} name={engine.name} about={text.about[engine.id]} focused={focus === engine.id} detailsLabel={text.versionAndFiles}
            status={state(profile?.engine_installed, ready, unsupported)} inUse={activeId === engine.id ? inUse : undefined}
            details={local || installedRevision || (runtime?.revision && runtime.revision !== installedRevision) ? <>
              {local && <p>{localExecutable} · {local}</p>}
              {installedRevision && <p>{managedPackage} · {installedRevision.slice(0, 12)}</p>}
              {runtime?.revision && runtime.revision !== installedRevision && <p>{t('download')} · {runtime.revision.slice(0, 12)}</p>}
            </> : undefined}>
            {unsupported && <p className="text-xs text-zinc-500 dark:text-zinc-400">{t('engineUnavailable')}</p>}
            {sets && profile && runtime?.supported && profile.engine_installed && profile.adapter_runtime_update &&
              <NativeMusicRuntimeInstall engine={engine.id} update onInstalled={refresh} />}
            {sets && !unsupported && <NativeEngineQuality engine={engine.id} name={engine.name} onChanged={() => void refresh()} />}
            {!sets && models && models.length > 0 && <ModelRows engine={engine.id} catalogue={engine.path} models={models} onChanged={() => void refresh()} />}
          </EngineCard>;
        })}
        {(data?.rvc?.supported !== false || showUnsupported || focus === 'rvc') &&
          <EngineCard id="rvc" name="RVC" about={text.about.rvc} focused={focus === 'rvc'} detailsLabel={text.versionAndFiles}
            status={state(data?.rvc?.installed, undefined, data?.rvc?.supported === false)}>
            {data?.rvc?.supported === false && data.rvc.reason && <p className="text-xs text-zinc-500 dark:text-zinc-400">{data.rvc.reason}</p>}
            {data?.rvc?.supported && !data.rvc.installed && <RvcNativeRuntime onInstalled={refresh} disabled={false} />}
          </EngineCard>}
      </ul>
      {data?.components && <p className="text-xs text-zinc-500 dark:text-zinc-400">
        {text.catalogue}: {catalogueText.list(data.components.sequence, new Date(data.components.published).toLocaleDateString(language))}
        {data.components.source === 'built-in' && <> · {catalogueText.builtIn}</>}
        {data.components.checked_at && <> · {catalogueText.checked(new Date(data.components.checked_at * 1000).toLocaleString(language))}</>}
      </p>}
    </section>
  );
}
