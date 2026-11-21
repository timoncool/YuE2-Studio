import { useEffect, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { rvcQuickStrings } from '../i18n/rvcQuick';
import { libraryChanged } from '../services/studioQueries';
import { poll } from '../services/poll';
import { readRvcBackend, restoreLastRvcVoice, RVC_QUICK_EVENT } from '../services/rvcQuick';
import { responseJson } from '../services/rvcVoices';

type Stage = 'queued' | 'separating' | 'converting' | 'completed' | 'cancelled' | 'failed';
interface Job {
  id: string;
  song_id: string;
  status: Stage;
  take: { song_id: string; effective_backend?: string } | null;
  error: string | null;
}

const URL = '/v1/rvc/quick';
const active = (job: Job) => job.status === 'queued' || job.status === 'separating' || job.status === 'converting';

export function RvcQuickJobs({ songs }: { songs: { id: string; title: string }[] }) {
  const { language, t } = useI18n();
  const labels = rvcQuickStrings[language];
  const [jobs, setJobs] = useState<Job[]>([]);
  const [dismissed, setDismissed] = useState<string[]>([]);
  const [error, setError] = useState<string>();

  useEffect(() => {
    const controller = new AbortController();
    void restoreLastRvcVoice(controller.signal);
    const knownTakes = new Set<string>();
    const stop = poll(async () => {
      try {
        const fetched = await responseJson<Job[]>(await fetch(URL, { signal: controller.signal }));
        if (!Array.isArray(fetched) || controller.signal.aborted) return;
        const added = fetched.flatMap(row => row.take && !knownTakes.has(row.take.song_id) ? [row.take.song_id] : []);
        added.forEach(id => knownTakes.add(id));
        if (added.length) void libraryChanged();
        setJobs(fetched);
      } catch { /* A short polling failure can recover on the next request. */ }
    }, 1800);

    function start(event: Event) {
      const { songId, voiceId } = (event as CustomEvent<{ songId: string; voiceId: string }>).detail;
      setError(undefined);
      void fetch(URL, { method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: songId, voice_id: voiceId, backend: readRvcBackend() }),
        signal: controller.signal,
      }).then(responseJson<Job>).then(job => {
        setDismissed(hidden => hidden.filter(id => id !== job.id));
        setJobs(rows => [job, ...rows.filter(row => row.id !== job.id)]);
        if (job.take) void libraryChanged();
      }).catch((failure: Error) => {
        if (!controller.signal.aborted) setError(failure.message);
      });
    }
    window.addEventListener(RVC_QUICK_EVENT, start);
    return () => {
      controller.abort();
      stop();
      window.removeEventListener(RVC_QUICK_EVENT, start);
    };
  }, []);

  const cancel = async (id: string) => {
    try {
      const stopped = await responseJson<Job>(await fetch(`${URL}/${encodeURIComponent(id)}`, { method: 'DELETE' }));
      setJobs(rows => rows.map(row => row.id === id ? stopped : row));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };
  const visible = jobs.filter(row => !dismissed.includes(row.id));
  const rows = [...visible.filter(active), ...visible.filter(row => !active(row)).slice(0, 3)];
  if (!rows.length && !error) return null;

  return <aside className="space-y-1 border-y border-zinc-200 px-4 py-2 text-xs dark:border-white/10" aria-label={labels.jobs}>
    {error && <div role="alert">{error} <button type="button" className="underline" onClick={() => setError(undefined)}>{t('dismiss')}</button></div>}
    {rows.map(job => (
      <div key={job.id} className="flex items-center gap-2">
        <p role="status">{songs.find(song => song.id === job.song_id)?.title ?? labels.jobs} · {labels[job.status]}{job.take?.effective_backend && ` · ${job.take.effective_backend}`}{job.error && ` · ${job.error}`}</p>
        {active(job) ? (
          <button type="button" className="underline" onClick={() => void cancel(job.id)}>{t('cancel')}</button>
        ) : (
          <button type="button" className="underline" onClick={() => setDismissed(hidden => [...hidden, job.id])}>{t('dismiss')}</button>
        )}
      </div>
    ))}
  </aside>;
}
