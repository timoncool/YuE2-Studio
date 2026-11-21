import { useEffect, useRef, useState } from 'react';
import { apiUrl } from './apiBase';

interface PackageFile {
  bytes: number;
}

export interface EnginePackage {
  id: string;
  name: string;
  version: string;
  platform: string;
  protocol: string;
  native_runtime?: { api_version: number };
  files: PackageFile[];
}

interface ModelPackage {
  id: string;
  name: string;
  version: string;
  engines: string[];
  files: PackageFile[];
}

export interface Catalogue {
  source?: string;
  platform: string;
  engines: EnginePackage[];
  models?: ModelPackage[];
}

interface Selection {
  model: string;
  engines: Record<string, string>;
}

const PREFS = 'studio.signedMusicSelection';

function preferences(): Selection {
  try {
    const saved = JSON.parse(localStorage.getItem(PREFS) ?? '{}') as Partial<Selection>;
    return { model: saved.model ?? '', engines: saved.engines ?? {} };
  } catch {
    return { model: '', engines: {} };
  }
}

interface PackageStatus {
  version: string;
}

export interface PackageProgress {
  stage: 'verifying' | 'downloading' | 'activating';
  file?: string | null;
  total_bytes: number;
  downloaded_bytes: number;
  reused_bytes: number;
}

interface Transfer {
  key: string;
  path: string;
}

async function packagePresence(path: string, signal: AbortSignal): Promise<PackageStatus | undefined> {
  const response = await fetch(apiUrl(`${path}?probe=presence`), { signal });
  if (response.status === 404 || response.status === 409) return undefined;
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

export async function postSignedPackage(path: string) {
  const response = await fetch(apiUrl(path), { method: 'POST' });
  if (!response.ok) {
    const error = await response.json().catch(() => undefined);
    throw new Error(error?.error ?? error?.message ?? `HTTP ${response.status}`);
  }
  return response;
}

export const signedPackageBytes = (files: PackageFile[]) => files.reduce((sum, file) => sum + file.bytes, 0);

export function useSignedMusicSelection(list: boolean, active: boolean) {
  const [selection, setSelection] = useState(preferences);
  const [catalogue, setCatalogue] = useState<Catalogue>();
  const [catalogueError, setCatalogueError] = useState<string>();
  const [refreshingCatalogue, setRefreshingCatalogue] = useState(false);
  const [probe, setProbe] = useState<{ key: string; engine?: PackageStatus; model?: PackageStatus; error?: string }>();
  const [transfer, setTransfer] = useState<Transfer>();
  const [progress, setProgress] = useState<PackageProgress>();
  const [cancelPending, setCancelPending] = useState(false);
  const operation = useRef<{ cancelled: boolean } | undefined>(undefined);
  const [installError, setInstallError] = useState<{ key: string; message: string }>();
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!list) return;
    const controller = new AbortController();
    void fetch(apiUrl('/v1/components/music-engines'), { signal: controller.signal }).then(async response => {
      if (!response.ok) return;
      const body: Catalogue = await response.json();
      if (!controller.signal.aborted && Array.isArray(body.engines)) setCatalogue(body);
    }).catch(() => { /* Existing built-in models remain available during service startup. */ });
    return () => controller.abort();
  }, [list]);
  const supported = catalogue?.engines.filter(engine =>
    engine.platform === catalogue.platform && engine.protocol === 'music-http-v1') ?? [];
  const models = (catalogue?.models ?? []).filter(model => supported.some(engine => model.engines.includes(engine.id)));
  const model = models.find(item => item.id === selection.model);
  const engines = supported.filter(engine => model?.engines.includes(engine.id));
  const engine = engines.find(item => item.id === selection.engines[selection.model]) ?? engines[0];
  const key = active && model && engine ? JSON.stringify([engine.id, model.id]) : undefined;
  const enginePath = engine && `/v1/components/music-engines/${encodeURIComponent(engine.id)}`;
  const modelPath = model && `/v1/components/music-models/${encodeURIComponent(model.id)}`;
  const modelStatus = model && enginePath && `${enginePath}/models/${encodeURIComponent(model.id)}/status`;

  useEffect(() => {
    if (!key || !enginePath || !modelStatus) return;
    const controller = new AbortController();
    void Promise.all([
      packagePresence(`${enginePath}/status`, controller.signal),
      packagePresence(modelStatus, controller.signal),
    ]).then(([engine, model]) => {
      if (!controller.signal.aborted) setProbe({ key, engine, model });
    }, (error: Error) => {
      if (!controller.signal.aborted) setProbe({ key, error: error.message });
    });
    return () => controller.abort();
  }, [key, enginePath, modelStatus, attempt]);

  const status = key && probe?.key === key ? probe : undefined;
  const needsEngine = Boolean(engine && status?.engine?.version !== engine.version);
  const needsModel = Boolean(model && status?.model?.version !== model.version);
  const ownTransfer = transfer?.key === key ? transfer : undefined;
  useEffect(() => {
    if (!ownTransfer) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const response = await fetch(apiUrl(`${ownTransfer.path}/progress`), { signal: controller.signal });
        if (!response.ok) return;
        const body: { active: PackageProgress | null } = await response.json();
        if (!controller.signal.aborted) setProgress(body.active ?? undefined);
      } catch { /* The install response reports the result even if progress is unavailable. */ }
      finally { if (!controller.signal.aborted) timer = setTimeout(() => void poll(), 400); }
    };
    setProgress(undefined);
    void poll();
    return () => { controller.abort(); clearTimeout(timer); };
  }, [ownTransfer]);
  const save = (next: Selection) => {
    setSelection(next);
    try { localStorage.setItem(PREFS, JSON.stringify(next)); } catch { /* Keep the session's selection. */ }
  };
  return {
    models, model, engines, engine,
    hasCatalogue: Boolean(catalogue?.platform),
    catalogueError,
    refreshingCatalogue,
    refreshCatalogue: async () => {
      if (refreshingCatalogue) return;
      setRefreshingCatalogue(true);
      setCatalogueError(undefined);
      try {
        const response = await postSignedPackage('/v1/components/music-engines/refresh');
        const body: Catalogue = await response.json();
        if (!Array.isArray(body.engines) || !body.platform) throw new Error('Invalid music engine catalogue');
        setCatalogue(body);
      } catch (error) {
        setCatalogueError(String(error));
      } finally {
        setRefreshingCatalogue(false);
      }
    },
    ready: Boolean(status?.engine && status?.model),
    pending: Boolean(key && !status),
    installing: Boolean(ownTransfer),
    busy: Boolean(transfer),
    progress: ownTransfer && progress,
    cancelPending,
    updateAvailable: Boolean(status?.engine && status?.model && (needsEngine || needsModel)),
    cancel: async () => {
      const run = operation.current;
      if (!ownTransfer || !progress || cancelPending || !run) return;
      setCancelPending(true);
      try {
        const response = await postSignedPackage(`${ownTransfer.path}/cancel`);
        const answer: { cancel_requested?: boolean } = await response.json();
        if (answer.cancel_requested) run.cancelled = true;
        else if (operation.current === run) setCancelPending(false);
      } catch (error) {
        if (operation.current !== run) return;
        setInstallError({ key: ownTransfer.key, message: String(error) });
        setCancelPending(false);
      }
    },
    error: status?.error ?? (installError?.key === key ? installError?.message : undefined),
    missingBytes: status && !status.error && engine && model
      ? (needsEngine ? signedPackageBytes(engine.files) : 0) + (needsModel ? signedPackageBytes(model.files) : 0)
      : undefined,
    selectModel: (id: string) => save({ ...selection, model: id }),
    selectPair: (model: string, engine: string) => save({ model, engines: { ...selection.engines, [model]: engine } }),
    selectEngine: (id: string) => {
      if (engines.some(engine => engine.id === id)) save({ ...selection, engines: { ...selection.engines, [selection.model]: id } });
    },
    refresh: () => setAttempt(current => current + 1),
    install: async () => {
      if (!key || !status || status.error || !enginePath || !modelPath || !engine || !model || operation.current) return;
      const run = { cancelled: false };
      operation.current = run;
      setInstallError(undefined);
      const installPackage = (kind: 'music-engines' | 'music-models', id: string, path: string) => {
        setProgress(undefined);
        setTransfer({ key, path: `/v1/components/packages/${kind}/${encodeURIComponent(id)}` });
        return postSignedPackage(`${path}/install`);
      };
      try {
        if (needsEngine) await installPackage('music-engines', engine.id, enginePath);
        if (needsModel && !run.cancelled) await installPackage('music-models', model.id, modelPath);
      } catch (error) {
        if (!run.cancelled && !String(error).includes('Package installation cancelled')) {
          setInstallError({ key, message: String(error) });
        }
      } finally {
        operation.current = undefined;
        setTransfer(undefined);
        setCancelPending(false);
        setAttempt(current => current + 1);
      }
    },
  };
}
