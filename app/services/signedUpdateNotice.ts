import { useEffect, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { SELF_READ } from './studioQueries';

interface UpdateNoticeStatus {
  mode: 'manual' | 'notify' | 'auto';
  notice_scope?: string;
  updates: { kind: string; id: string; from_version: string; to_version: string; automatic_updates_paused?: boolean }[];
}

const eventName = 'yue:signed-package-updates-changed';
const storageKey = 'yue:signed-package-notices';
const noticeHistorySize = 256;

function readNoticeHistory(): string[] {
  try {
    const entries: unknown = JSON.parse(window.localStorage.getItem(storageKey) ?? '[]');
    if (Array.isArray(entries)) return entries.filter(entry => typeof entry === 'string');
  } catch {
    // Storage may be unavailable in restricted webviews.
  }
  return [];
}

export const signedUpdateNoticeChanged = () => window.dispatchEvent(new Event(eventName));

export function useSignedUpdateNotice(online: boolean, revision: string | undefined, canShow: boolean, announce: () => void) {
  const { data, refetch } = useQuery({
    queryKey: ['signed-component-update-notice', revision],
    queryFn: async ({ signal }): Promise<UpdateNoticeStatus | null> => {
      const response = await fetch('/v1/components/updates', { headers: SELF_READ, signal });
      return response.ok ? response.json() : null;
    },
    enabled: online,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
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
    const scope = data.notice_scope;
    const history = new Set(scope ? readNoticeHistory() : []);
    const pending = data.updates.filter(update => !update.automatic_updates_paused)
      .map(update => JSON.stringify([scope, update.kind, update.id, update.to_version]));
    const newNotices = pending.filter(key => !seen.current.has(key) && !history.has(key));
    if (!newNotices.length) return;
    newNotices.forEach(key => seen.current.add(key));
    if (scope) {
      newNotices.forEach(key => history.add(key));
      try {
        window.localStorage.setItem(storageKey, JSON.stringify([...history].slice(-noticeHistorySize)));
      } catch {
        // The in-memory set still suppresses duplicates until the window closes.
      }
    }
    announce();
  }, [online, canShow, data, announce]);
}
