import React, { useCallback, useEffect, useState } from 'react';
import { apiUrl } from '../services/apiBase';
import type { NativeProfile } from '../services/nativeMusicProfiles';
import { NativeModelSetPicker } from './NativeModelSetPicker';

/**
 * YuE2's sets in the same quality card ACE-Step and MiniMax use: the studio's setup holds them (its catalogue, what
 * is on disk, the running download), and a set becomes the studio's once its files are all there.
 */

type CatalogProfile = { id: string; label: string; labels?: Record<string, string>; vram_gb?: number; advanced?: boolean; components: string[]; total_bytes: number };
type Catalog = { recommended_profile_id?: string; profiles: CatalogProfile[]; components?: { id: string; partial_bytes?: number }[] };
type Status = {
  selected_profile_id?: string | null;
  installed_components?: string[];
  components?: { id: string; bytes: number }[];
  active?: { status: string } | null;
  hardware?: { recommended?: string };
};

async function read<T>(path: string): Promise<T> {
  const response = await fetch(apiUrl(path));
  if (!response.ok) throw new Error(`${path} answered ${response.status}`);
  return (await response.json()) as T;
}

/** `manage` adds deleting a set and a half-finished download, for Settings; the create form only chooses. */
export function YueModelSetPicker({ onChanged, manage = false }: { onChanged: () => void; manage?: boolean }) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [nextCatalog, nextStatus] = await Promise.all([read<Catalog>('/setup/catalog'), read<Status>('/setup/status')]);
      setCatalog(nextCatalog);
      setStatus(nextStatus);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, []);
  useEffect(() => { void load(); }, [load]);

  const installed = new Set(status?.installed_components ?? []);
  const bytes = new Map((status?.components ?? []).map(component => [component.id, component.bytes]));
  const recommended = status?.hardware?.recommended ?? catalog?.recommended_profile_id;
  const profiles: NativeProfile[] = (catalog?.profiles ?? []).map(profile => {
    const missing = profile.components.filter(id => !installed.has(id));
    return {
      id: profile.id, label: profile.label, labels: profile.labels, vram_gb: profile.vram_gb, advanced: profile.advanced,
      recommended: profile.id === recommended, total_bytes: profile.total_bytes, components: profile.components,
      missing, missing_bytes: missing.reduce((sum, id) => sum + (bytes.get(id) ?? 0), 0), ready: missing.length === 0,
      installing: status?.active?.status === 'downloading',
    };
  });
  const selectedId = chosen ?? status?.selected_profile_id ?? recommended;
  const selected = profiles.find(profile => profile.id === selectedId);

  // a set the setup does not hold yet stays chosen in the window until its download finishes
  const settle = async (id: string) => {
    const profile = catalog?.profiles.find(entry => entry.id === id);
    const onDisk = new Set((await read<Status>('/setup/status')).installed_components ?? []);
    if (!profile || !profile.components.every(component => onDisk.has(component))) return;
    const response = await fetch(apiUrl('/setup/select'), {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ profile_id: id }),
    });
    if (!response.ok) {
      const body = await response.json().catch(() => null) as { error?: string } | null;
      setError(body?.error ?? String(response.status));
      return;
    }
    setChosen(null);
    await load();
    onChanged();
  };

  // files half-way through a download, per set: they can be resumed by choosing the set, or deleted
  const partialIds = (id: string) => {
    const profile = catalog?.profiles.find(entry => entry.id === id);
    return (profile?.components ?? []).filter(component => !installed.has(component)
      && (catalog?.components?.find(entry => entry.id === component)?.partial_bytes ?? 0) > 0);
  };
  const remove = async (ids: string[]) => {
    if (!ids.length) return;
    const response = await fetch(apiUrl('/setup/remove'), {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ids }),
    });
    if (!response.ok) {
      const body = await response.json().catch(() => null) as { error?: string } | null;
      setError(body?.error ?? String(response.status));
      return;
    }
    await load();
    onChanged();
  };

  if (!catalog) return error ? <p role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</p> : null;
  return (
    <>
      <NativeModelSetPicker engine="yue2-cpp" name="YuE2" profiles={profiles} selected={selected} engineInstalled
        onSelect={id => { setChosen(id); if (profiles.find(entry => entry.id === id)?.ready) void settle(id); }}
        onChanged={() => { void load(); onChanged(); }}
        onReady={() => { if (selectedId) void settle(selectedId); }}
        onRemove={manage ? id => void remove(catalog.profiles.find(entry => entry.id === id)?.components ?? []) : undefined}
        partial={id => partialIds(id).length > 0}
        onDiscardPartial={manage ? id => void remove(partialIds(id)) : undefined} />
      {error && <p role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</p>}
    </>
  );
}
