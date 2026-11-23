import { useEffect, useState } from 'react';
import newsData from '../data/news.json';
import { useHubState } from './studioQueries';
import type { HubItem } from './studioHub';

const SEEN_KEY = 'yue2.news.seen';
const CHANGED = 'yue2:news-seen';

function seenIds(): Set<string> {
  try {
    const raw = localStorage.getItem(SEEN_KEY);
    return new Set(raw ? JSON.parse(raw) as string[] : []);
  } catch {
    return new Set();
  }
}

/** Every news item the window can show, by the ids the news page uses: the hub's first, then the bundled ones. */
export function useNewsIds(language: string): string[] {
  const hub = useHubState(language);
  const hubIds = (hub.data?.items ?? []).filter((item: HubItem) => item.kind === 'news').map((item: HubItem) => `hub:${item.id}`);
  return [...hubIds, ...(newsData as { id: string }[]).map(item => item.id)];
}

/** How many news items appeared since the news page was last opened. */
export function useUnseenNews(language: string): number {
  const ids = useNewsIds(language);
  const [seen, setSeen] = useState(seenIds);
  useEffect(() => {
    const changed = () => setSeen(seenIds());
    window.addEventListener(CHANGED, changed);
    return () => window.removeEventListener(CHANGED, changed);
  }, []);
  // a first launch counts what it already ships as read: the badge is for what arrives later
  const first = (() => { try { return localStorage.getItem(SEEN_KEY) === null; } catch { return false; } })();
  useEffect(() => { if (first && ids.length) markNewsSeen(ids); }, [first, ids.length]);
  return first ? 0 : ids.filter(id => !seen.has(id)).length;
}

/** The news page was opened: what it shows counts as seen. */
export function markNewsSeen(ids: string[]): void {
  try {
    const next = seenIds();
    for (const id of ids) next.add(id);
    localStorage.setItem(SEEN_KEY, JSON.stringify([...next]));
  } catch {
    return;
  }
  window.dispatchEvent(new Event(CHANGED));
}
