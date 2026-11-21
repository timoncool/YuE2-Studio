import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useI18n } from '../context/I18nContext';
import { enginePageStrings, showUnsupportedEngines } from '../i18n/enginePage';
import { apiUrl } from '../services/apiBase';
import type { PairStatus } from '../services/musicPair';
import { SELF_READ, useOpenRouterSettings, useSetupStatus } from '../services/studioQueries';
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
type ModelStatus = { id: string; label?: string; name?: string; ready: boolean; missing_bytes?: number };
type ProfileStatus = { engine_installed: boolean; adapter_runtime_update?: boolean; profiles?: ModelStatus[]; models?: ModelStatus[] };
type RuntimeStatus = { supported: boolean; installed?: boolean; revision?: string };
type RvcStatus = { installed: boolean; supported: boolean; reason?: string | null };
const installable = ['heartmula', 'stable_audio', 'soulx_singer', 'soulx_singer_svc', 'yue2', 'ace_step',
  'stable-audio-3-medium', 'heartmula-3b-q8_0', 'hotstep-minimax-music3', 'hotstep-ace-step-1.5', 'hotstep-yue2-q4-k-m'] as const;

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
  const setup = useSetupStatus<{ ready?: boolean; engine_ready?: boolean }>();
  const cloud = useOpenRouterSettings<{ configured?: boolean }>();
  const catalogue = useQuery({
    queryKey: ['engine-page'],
    queryFn: async ({ signal }) => {
      const [pair, local, rvc, profiles, ace, minimax] = await Promise.all([
        read<PairStatus>('/v1/music/pair', signal),
        read<{ executable?: string | null; executable_source?: string; in_effect?: { backend?: string } }>('/engine/options', signal),
        read<RvcStatus>('/v1/rvc/native/runtime', signal),
        Promise.all(native.map(engine => read<ProfileStatus>(engine.path, signal))),
        read<RuntimeStatus>('/v1/music/native/acestep-cpp/runtime', signal),
        read<RuntimeStatus>('/v1/music/native/minimaxmusic-cpp/runtime', signal),
      ]);
      return { pair, local, rvc, profiles, ace, minimax };
    },
    staleTime: 0,
  });
  const data = catalogue.data;
  const hasUnsupported = data?.rvc?.supported === false
    || (data?.ace?.supported === false && data.profiles[0]?.engine_installed === false)
    || (data?.minimax?.supported === false && data.profiles[1]?.engine_installed === false);
  const refresh = () => { void catalogue.refetch(); };
  const status = (value: boolean | undefined) => value === undefined ? (catalogue.isPending ? checking : unavailable) : value ? installed : missing;
  const mark = (id: string) => id === focus ? ' border-pink-500' : ' border-zinc-200 dark:border-white/10';
  const action = 'text-sm text-pink-600 underline dark:text-pink-300';
  const activePair = data?.pair?.active;
  const activeId = activePair?.kind === 'native' ? activePair.engine : activePair?.kind === 'audio_cpp' ? 'audio.cpp' : activePair?.kind;
  return (
    <section aria-label={title} className="space-y-5">
      {focus === 'updates' && <UpdatePlanView />}
      <div className="flex flex-wrap items-center gap-3">
        <button type="button" className={action} disabled={catalogue.isFetching} onClick={refresh}>{t('refresh')}</button>
        {hasUnsupported && <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={showUnsupported} onChange={event => setShowUnsupported(event.target.checked)} />
          {showUnsupportedEngines[language]}
        </label>}
      </div>
      <ul ref={list} className="grid gap-2" aria-label={title}>
        <li data-engine="local" className={'space-y-1 rounded-lg border p-3' + mark('local')}>
          <h4 className="font-semibold">yue2.cpp</h4>
          <p>{status(data?.local ? Boolean(data.local.executable || setup.data?.engine_ready) : undefined)}
            {setup.data?.engine_ready && <> · {active}{data?.local?.in_effect?.backend && ' · ' + data.local.in_effect.backend}</>}
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
          const unsupported = runtime?.supported === false && profile?.engine_installed === false;
          if (unsupported && !showUnsupported && focus !== engine.id) return null;
          return <li key={engine.id} data-engine={engine.id} aria-label={engine.name} className={'space-y-1 rounded-lg border p-3' + mark(engine.id)}>
            <h4 className="font-semibold">{engine.name}</h4>
            <p>{status(profile?.engine_installed)}{activeId === engine.id && ' · ' + active}
              {ready !== undefined && <> · {t(ready ? 'profileReady' : 'profileNotInstalled')}</>}</p>
            {unsupported && <p className="text-sm">{t('engineUnavailable')}</p>}
            {runtime?.revision && <p className="text-xs">{runtime.installed ? installed : t('download')} · <code>{runtime.revision.slice(0, 12)}</code></p>}
            {(engine.id === 'acestep-cpp' || engine.id === 'minimaxmusic-cpp') && profile &&
              runtime?.supported && (!profile.engine_installed || profile.adapter_runtime_update) &&
              <InstallDetails key={focus} label={t('download')} initialOpen={focus === engine.id}>
                <NativeMusicRuntimeInstall engine={engine.id} update={profile.adapter_runtime_update} onInstalled={refresh} />
              </InstallDetails>}
            {(engine.id === 'hotstep' || engine.id === 'audio.cpp') && models?.map(model => {
              const id = installable.find(value => value === model.id);
              return id && !model.ready ? <InstallDetails key={id} label={model.label ?? model.name ?? id}>
                <AudioCppModelInstall model={id} bytes={model.missing_bytes ?? 0} language={language} onInstalled={refresh} />
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
