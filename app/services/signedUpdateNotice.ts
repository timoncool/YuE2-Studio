import { useEffect, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { SELF_READ } from './studioQueries';

interface UpdateNoticeStatus {
  mode: 'manual' | 'notify' | 'auto';
  updates: { kind: string; id: string; from_version: string; to_version: string; automatic_updates_paused?: boolean }[];
}

const eventName = 'yue:signed-package-updates-changed';

export const signedUpdateNoticeChanged = () => window.dispatchEvent(new Event(eventName));

export function useSignedUpdateNotice(online: boolean, canShow: boolean, announce: () => void) {
  const { data, refetch } = useQuery({
    queryKey: ['signed-component-update-notice'],
    queryFn: async ({ signal }): Promise<UpdateNoticeStatus | null> => {
      const response = await fetch('/v1/components/updates', { headers: SELF_READ, signal });
      return response.ok ? response.json() : null;
    },
    enabled: online,
    refetchInterval: query => query.state.data?.mode === 'notify' ? 30_000 : false,
    refetchOnWindowFocus: true,
  });
  const seen = useRef(new Set<string>());
  useEffect(() => {
    if (!online) return;
    const changed = () => { void refetch(); };
    window.addEventListener(eventName, changed);
    return () => window.removeEventListener(eventName, changed);
  }, [online, refetch]);
  useEffect(() => {
    if (!online || !canShow || data?.mode !== 'notify' || !Array.isArray(data.updates)) return;
    const pending = data.updates.filter(update => !update.automatic_updates_paused)
      .map(update => JSON.stringify([update.kind, update.id, update.from_version, update.to_version]));
    if (pending.every(key => seen.current.has(key))) return;
    pending.forEach(key => seen.current.add(key));
    announce();
  }, [online, canShow, data, announce]);
}
