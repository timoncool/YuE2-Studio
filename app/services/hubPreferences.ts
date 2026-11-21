import { useEffect, useState } from 'react';

/** Local, opt-out-of-promotions preference. This does not change anonymous telemetry
 * or the News page; it only stops unsolicited Studio Hub bars and popups. */
export const HUB_PROMOTIONS_KEY = 'studio.hub.showPromotions';
const CHANGED = 'studio:hub-promotions-changed';

export function showHubPromotions(): boolean {
  try { return window.localStorage.getItem(HUB_PROMOTIONS_KEY) !== '0'; }
  catch { return true; }
}

/** Hide unsolicited banners/popups while retaining news opened by the user. */
export function filterHubPromotions<T extends { kind: string }>(items: T[], show: boolean): T[] {
  return show ? items : items.filter(item => item.kind === 'news');
}

export function setHubPromotions(show: boolean): void {
  window.localStorage.setItem(HUB_PROMOTIONS_KEY, show ? '1' : '0');
  window.dispatchEvent(new Event(CHANGED));
}

/** A preference change in Settings takes effect immediately in the open window. */
export function useHubPromotions(): [boolean, (show: boolean) => void] {
  const [show, setShow] = useState(showHubPromotions);
  useEffect(() => {
    const changed = () => setShow(showHubPromotions());
    window.addEventListener(CHANGED, changed);
    window.addEventListener('storage', changed);
    return () => {
      window.removeEventListener(CHANGED, changed);
      window.removeEventListener('storage', changed);
    };
  }, []);
  return [show, setHubPromotions];
}
