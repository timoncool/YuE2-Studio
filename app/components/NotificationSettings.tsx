import React, { useState } from 'react';
import { BellRing } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { sendWindowsSongNotification, useSongNotifications } from '../services/songNotifications';

/** A local desktop preference, independent of promotional notices and telemetry. */
export const NotificationSettings: React.FC = () => {
  const { t } = useI18n();
  const [enabled, setEnabled] = useSongNotifications();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const test = async () => {
    setBusy(true);
    setResult(null);
    try {
      const sent = await sendWindowsSongNotification(t('shellNotificationsTestBody'));
      setResult(t(sent ? 'shellNotificationsTestRequested' : 'shellNotificationsUnavailable'));
    } finally { setBusy(false); }
  };
  return (
    <section className="max-w-xl space-y-4 text-sm">
      <p className="text-zinc-500 dark:text-zinc-400">{t('shellNotificationsWhy')}</p>
      <label className="flex cursor-pointer items-center gap-3">
        <input type="checkbox" checked={enabled} onChange={event => {
          setEnabled(event.target.checked);
          setResult(null);
        }} className="h-4 w-4 accent-pink-500" />
        <span className="font-semibold">{t('shellNotificationsEnabled')}</span>
      </label>
      <button type="button" disabled={!enabled || busy} onClick={() => void test()}
        className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 bg-zinc-100 px-3 py-2 font-medium hover:bg-zinc-200 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/10 dark:bg-white/10 dark:hover:bg-white/15">
        <BellRing size={16} aria-hidden="true" />{t('shellNotificationsTest')}
      </button>
      {result && <p role="status" className="text-xs text-zinc-500 dark:text-zinc-400">{result}</p>}
    </section>
  );
};
