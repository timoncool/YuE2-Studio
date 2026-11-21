import { useEffect, useRef, useState } from 'react';
import { apiUrl } from './apiBase';

interface PackageFile {
  bytes: number;
}

interface EnginePackage {
  id: string;
  name: string;
  platform: string;
  protocol: string;
  files: PackageFile[];
}

interface ModelPackage {
  id: string;
  name: string;
  engines: string[];
  files: PackageFile[];
}

interface Catalogue {
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

async function available(path: string, signal: AbortSignal): Promise<boolean> {
  const response = await fetch(apiUrl(path), { signal });
  if (response.status === 404 || response.status === 409) return false;
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return true;
}

async function post(path: string) {
  const response = await fetch(apiUrl(path), { method: 'POST' });
  if (!response.ok) {
    const error = await response.json().catch(() => undefined);
    throw new Error(error?.error ?? error?.message ?? `HTTP ${response.status}`);
  }
  return response;
}

const bytes = (files: PackageFile[]) => files.reduce((sum, file) => sum + file.bytes, 0);

export function useSignedMusicSelection(list: boolean, active: boolean) {
  const [selection, setSelection] = useState(preferences);
  const [catalogue, setCatalogue] = useState<Catalogue>();
  const [catalogueError, setCatalogueError] = useState<string>();
  const [refreshingCatalogue, setRefreshingCatalogue] = useState(false);
  const [probe, setProbe] = useState<{ key: string; engine?: boolean; model?: boolean; error?: string }>();
  const [installing, setInstalling] = useState<string>();
  const installLock = useRef(false);
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
    setProbe(undefined);
    void Promise.all([
      available(`${enginePath}/status`, controller.signal),
      available(modelStatus, controller.signal),
    ]).then(([engine, model]) => {
      if (!controller.signal.aborted) setProbe({ key, engine, model });
    }, (error: Error) => {
      if (!controller.signal.aborted) setProbe({ key, error: error.message });
    });
    return () => controller.abort();
  }, [key, enginePath, modelStatus, attempt]);

  const status = key && probe?.key === key ? probe : undefined;
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
        const response = await post('/v1/components/music-engines/refresh');
        const body: Catalogue = await response.json();
        if (!Array.isArray(body.engines) || !body.platform) throw new Error('Invalid music engine catalogue');
        setCatalogue(body);
      } catch (error) {
        setCatalogueError(String(error));
      } finally {
        setRefreshingCatalogue(false);
      }
    },
    ready: status?.engine === true && status.model === true,
    pending: Boolean(key && !status),
    installing: Boolean(installing),
    error: status?.error ?? (installError?.key === key ? installError?.message : undefined),
    missingBytes: status && !status.error && engine && model
      ? (status.engine ? 0 : bytes(engine.files)) + (status.model ? 0 : bytes(model.files))
      : undefined,
    selectModel: (id: string) => save({ ...selection, model: id }),
    selectEngine: (id: string) => {
      if (engines.some(engine => engine.id === id)) save({ ...selection, engines: { ...selection.engines, [selection.model]: id } });
    },
    refresh: () => setAttempt(current => current + 1),
    install: async () => {
      if (!key || !status || status.error || !enginePath || !modelPath || installLock.current) return;
      installLock.current = true;
      setInstallError(undefined);
      setInstalling(key);
      try {
        if (!status.engine) await post(`${enginePath}/install`);
        if (!status.model) await post(`${modelPath}/install`);
        setAttempt(current => current + 1);
      } catch (error) {
        setInstallError({ key, message: String(error) });
      } finally {
        installLock.current = false;
        setInstalling(undefined);
      }
    },
  };
}
