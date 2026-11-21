import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useI18n } from '../context/I18nContext';
import { enginePageStrings, runtimeSourceStrings, showUnsupportedEngines } from '../i18n/enginePage';
import { apiUrl } from '../services/apiBase';
import type { PairStatus } from '../services/musicPair';
import { SELF_READ, useOpenRouterSettings, useSetupStatus } from '../services/studioQueries';
import { postSignedPackage, type Catalogue } from '../services/signedMusicSelection';
import { SignedNativeRuntimeInstall } from './SignedNativeRuntimeInstall';
import { NativeMusicRuntimeInstall } from './NativeMusicRuntimeInstall';
import { AudioCppModelInstall } from './HeartMuLaControls';
import { UpdatePlanView } from './UpdateScreen';
import { RvcNativeRuntime } from './RvcNativeRuntime';

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
  selected_executable?: { path: string; source: 'local' | 'managed' | 'signed'; package?: { version: string } } | null;
  profiles?: ModelStatus[];
  models?: ModelStatus[];
};
type RuntimeStatus = { supported: boolean; installed?: boolean; revision?: string; installed_revision?: string | null };
type RvcStatus = { installed: boolean; supported: boolean; reason?: string | null };
function modelInstallPath(engine: string, path?: string | null) {
  if (engine === 'audio.cpp' && path === '/v1/music/audio-cpp/models/install') return path;
  if (engine === 'hotstep' && path && /^\/v1\/music\/native\/hotstep\/(?:[a-z0-9-]+\/)?install$/.test(path)) return path;
}

function InstallDetails({ label, initialOpen = false, children }: { label: string; initialOpen?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(initialOpen);
  return <details open={open} onToggle={event => setOpen(event.currentTarget.open)}>
    <summary className="cursor-pointer text-sm">{label}</summary>
    {open && <div className="mt-2 space-y-2">{children}</div>}
  </details>;
}

async function read<T>(path: string, signal: AbortSignal): Promise<T | null> {
  try {
    const response = await fetch(apiUrl(path), { signal, headers: SELF_READ });
    return response.ok ? await response.json() : null;
  } catch {
    return null;
  }
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
  const setup = useSetupStatus<{ ready?: boolean; engine_ready?: boolean }>();
  const cloud = useOpenRouterSettings<{ configured?: boolean }>();
  const catalogue = useQuery({
    queryKey: ['engine-page'],
    queryFn: async ({ signal }) => {
      const [pair, local, rvc, profiles, ace, minimax, signed] = await Promise.all([
        read<PairStatus>('/v1/music/pair', signal),
        read<{ executable?: string | null; executable_source?: string; in_effect?: { backend?: string } }>('/engine/options', signal),
        read<RvcStatus>('/v1/rvc/native/runtime', signal),
        Promise.all(native.map(engine => read<ProfileStatus>(engine.path, signal))),
        read<RuntimeStatus>('/v1/music/native/acestep-cpp/runtime', signal),
        read<RuntimeStatus>('/v1/music/native/minimaxmusic-cpp/runtime', signal),
        read<Catalogue>('/v1/components/music-engines', signal),
      ]);
      return { pair, local, rvc, profiles, ace, minimax, signed };
    },
    staleTime: 0,
    refetchInterval: query => query.state.data?.signed?.source === 'bundled' ? 2_000 : false,
  });
  const data = catalogue.data;
  const [refreshingSigned, setRefreshingSigned] = useState(false);
  const [signedError, setSignedError] = useState('');
  const requestedBootstrap = useRef(false);
  async function refreshSigned() {
    setRefreshingSigned(true);
    setSignedError('');
    try {
      await postSignedPackage('/v1/components/music-engines/refresh');
    } catch (error) {
      setSignedError(error instanceof Error ? error.message : String(error));
    } finally {
      setRefreshingSigned(false);
      void catalogue.refetch();
    }
  }
  const hasUnsupported = data?.rvc?.supported === false
    || (data?.ace?.supported === false && data.profiles[0]?.engine_installed === false)
    || (data?.minimax?.supported === false && data.profiles[1]?.engine_installed === false);
  const refresh = async () => { await Promise.all([catalogue.refetch(), setup.refetch(), cloud.refetch()]); };
  const status = (value: boolean | undefined) => value === undefined ? (catalogue.isPending ? checking : unavailable) : value ? installed : missing;
  const mark = (id: string) => id === focus ? ' border-pink-500' : ' border-zinc-200 dark:border-white/10';
  const action = 'text-sm text-pink-600 underline dark:text-pink-300';
  const activePair = data?.pair?.active;
  const activeId = activePair?.kind === 'native' ? activePair.engine : activePair?.kind === 'audio_cpp' ? 'audio.cpp' : activePair?.kind;
  return (
    <section aria-label={title} className="space-y-5">
      {focus === 'updates' && <UpdatePlanView />}
      <div className="flex flex-wrap items-center gap-3">
        <button type="button" className={action} disabled={catalogue.isFetching || setup.isFetching || cloud.isFetching || refreshingSigned} onClick={() => {
          if (data?.signed?.platform && !refreshingSigned) void refreshSigned();
          void refresh();
        }}>{t('refresh')}</button>
        {hasUnsupported && <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={showUnsupported} onChange={event => setShowUnsupported(event.target.checked)} />
          {showUnsupportedEngines[language]}
        </label>}
      </div>
      {signedError && <p role="alert" className="text-sm">{signedError}</p>}
      <ul ref={list} className="grid gap-2" aria-label={title}>
        <li data-engine="local" className={'space-y-1 rounded-lg border p-3' + mark('local')}>
          <h4 className="font-semibold">yue2.cpp</h4>
          <p>{status(data?.local ? Boolean(data.local.executable || setup.data?.engine_ready) : undefined)}
            {activeId === 'local' && <> · {active}{data?.local?.in_effect?.backend && ' · ' + data.local.in_effect.backend}</>}
            {setup.data?.ready !== undefined && <> · {t(setup.data.ready ? 'profileReady' : 'profileNotInstalled')}</>}
          </p>
          {data?.local?.executable && <p className="break-all text-xs">{data.local.executable}
            {data.local.executable_source === 'environment' && <> · {t('engineBinEnvOverride')}</>}
          </p>}
          <button type="button" className={action} onClick={() => onNavigate('models')}>{manageModels}</button>
          {' · '}<button type="button" className={action} onClick={() => onNavigate('engine')}>{settings}</button>
        </li>
        {native.map((engine, index) => {
          const profile = data?.profiles[index];
          const models = profile?.profiles ?? profile?.models;
          const ready = models?.some(model => model.ready);
          const runtime = engine.id === 'acestep-cpp' ? data?.ace : engine.id === 'minimaxmusic-cpp' ? data?.minimax : null;
          const installedRevision = runtime?.installed_revision ?? (runtime?.installed ? runtime.revision : undefined);
          const firstParty = (engine.id === 'acestep-cpp' || engine.id === 'minimaxmusic-cpp') && data?.signed?.source === 'signed'
            ? data.signed.engines?.find(item => item.id === engine.id && item.platform === data.signed?.platform
              && item.protocol === 'music-native-windows-v2' && item.native_runtime?.api_version === 2)
            : undefined;
          const selectedSource = profile?.selected_executable?.source;
          const signedInstall = profile && firstParty && selectedSource !== 'signed' && selectedSource !== 'local';
          const unsupported = !firstParty && runtime?.supported === false && profile?.engine_installed === false;
          if (unsupported && !showUnsupported && focus !== engine.id) return null;
          return <li key={engine.id} data-engine={engine.id} aria-label={engine.name} className={'space-y-1 rounded-lg border p-3' + mark(engine.id)}>
            <h4 className="font-semibold">{engine.name}</h4>
            <p>{status(profile?.engine_installed)}{activeId === engine.id && ' · ' + active}
              {ready !== undefined && <> · {t(ready ? 'profileReady' : 'profileNotInstalled')}</>}</p>
            {unsupported && <p className="text-sm">{t('engineUnavailable')}</p>}
            {profile?.selected_executable?.source === 'local' &&
              <p className="break-all text-xs">{localExecutable} · <code>{profile.selected_executable.path}</code></p>}
            {profile?.selected_executable?.source === 'signed' &&
              <p className="break-all text-xs">{managedPackage} · {profile.selected_executable.package?.version} · <code>{profile.selected_executable.path}</code></p>}
            {installedRevision && <p className="text-xs">{managedPackage} · {installed} · <code>{installedRevision.slice(0, 12)}</code></p>}
            {!firstParty && runtime?.revision && runtime.revision !== installedRevision &&
              <p className="text-xs">{t('download')} · <code>{runtime.revision.slice(0, 12)}</code></p>}
            {signedInstall && <SignedNativeRuntimeInstall key={firstParty.version} pack={firstParty} onInstalled={refresh} />}
            {!signedInstall && !firstParty && (engine.id === 'acestep-cpp' || engine.id === 'minimaxmusic-cpp') && profile &&
              runtime?.supported && (!profile.engine_installed || profile.adapter_runtime_update) &&
              <InstallDetails key={focus} label={t('download')} initialOpen={focus === engine.id}>
                <NativeMusicRuntimeInstall engine={engine.id} update={profile.adapter_runtime_update} onInstalled={refresh} />
              </InstallDetails>}
            {models?.map(model => {
              const installPath = modelInstallPath(engine.id, model.install_path);
              const label = model.label ?? model.name ?? model.id;
              return installPath && !model.ready ? <InstallDetails key={model.id} label={label}>
                <AudioCppModelInstall model={model.id} bytes={model.missing_bytes ?? 0} language={language} onInstalled={refresh}
                  target={{ catalogue: engine.path, installPath, label }} />
              </InstallDetails> : null;
            })}
          </li>;
        })}
        {(data?.rvc?.supported !== false || showUnsupported || focus === 'rvc') &&
        <li data-engine="rvc" className={'space-y-1 rounded-lg border p-3' + mark('rvc')}>
          <h4 className="font-semibold">RVC · vocal-fast-rs</h4>
          <p>{data?.rvc?.supported === false ? data.rvc.reason || unavailable : status(data?.rvc?.installed)}</p>
          {data?.rvc?.supported && !data.rvc.installed &&
            <InstallDetails label={t('download')}><RvcNativeRuntime onInstalled={refresh} disabled={false} /></InstallDetails>}
        </li>}
        <li data-engine="openrouter" className={'space-y-1 rounded-lg border p-3' + mark('openrouter')}>
          <h4 className="font-semibold">OpenRouter</h4>
          <p>{cloud.data ? t(cloud.data.configured ? 'openRouterConfigured' : 'openRouterNotConfigured') : cloud.isPending ? checking : unavailable}</p>
          <button type="button" className={action} onClick={() => onNavigate('cloud')}>{t('providers')}</button>
        </li>
      </ul>
      {focus !== 'updates' && <UpdatePlanView />}
    </section>
  );
}
