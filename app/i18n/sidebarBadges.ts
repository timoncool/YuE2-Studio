import type { Language } from './translations';

/** What the numbers on the sidebar say to a screen reader. */
export const sidebarBadgeStrings: Record<Language, { updates: (count: number) => string; news: (count: number) => string }> = {
  en: { updates: count => `${count} to update`, news: count => `${count} new` },
  ru: { updates: count => `обновлений: ${count}`, news: count => `новых: ${count}` },
  zh: { updates: count => `${count} 项可更新`, news: count => `${count} 条新消息` },
  ja: { updates: count => `更新 ${count} 件`, news: count => `新着 ${count} 件` },
  ko: { updates: count => `업데이트 ${count}개`, news: count => `새 소식 ${count}개` },
  pt: { updates: count => `${count} para atualizar`, news: count => `${count} novas` },
};
