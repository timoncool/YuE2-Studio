import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useI18n } from '../context/I18nContext';
import { songErrorText } from '../i18n/songErrors';
import type { Song, YueJob, YueProgress, YueRequest } from '../types';
import { followEngineProgress, stageDetail, type StageClock } from './engineProgress';
import { mapNativeLibrarySong, setNativeSongNote } from './nativeLibrary';
import { hubStateChanged, playlistsChanged, queryClient, readJson, updateLibraryPlaylists, updateLibrarySongs, useLibrarySongs } from './studioQueries';

/**
 * The songs being made, as cards in the list. A card follows its engine job
 * and, when the job is done, hands its row to the song the job made: the row
 * keeps its React key and its place and only changes what it shows (the view
 * key of TanStack DB's temporary ids), so nothing closes and reopens.
 */

const activeJobsKey = ['music', 'jobs'] as const;
const endedJobsKey = ['music', 'jobs', 'ended'] as const;
const ENDED_PREFIX = 'ended_';
const POLL_MS = 1_500;
const NO_SONGS: Song[] = [];

const STAGE_LABEL: Record<YueProgress['stage'], string> = {
  score: 'stageScore',
  semantic: 'stageSemanticProgress',
  acoustic: 'stageAcoustic',
  decode: 'stageDecode',
  transcribe: 'stageTranscribe',
};

type Notify = (message: string, type: 'success' | 'error' | 'info') => void;


interface GenerationOptions {
  /** The engine is up; before that there are no jobs to follow. */
  enabled: boolean;
  notify: Notify;
  /** A card became the songs its job made, the first of them in its row. */
  onFinished: (cardId: string, songs: Song[]) => void;
}

interface CardFields {
  title: string;
  style?: string;
  lyrics?: string;
  jobId?: string;
  createdAt?: Date;
  /** The playlist the songs go into. */
  playlistId?: string;
}

/** A note written on a song while it is still being made, by its card's id: the finished songs take it. */
export const pendingNotes = new Map<string, string>();

/** Hands a card's note to the songs its job made, once: the job's answer and the library's refresh may both bring them. */
function takePendingNote(cardId: string, made: Song[], onError: (message: string) => void): string | undefined {
  const note = pendingNotes.get(cardId)?.trim();
  pendingNotes.delete(cardId);
  if (!note || made.length === 0) return undefined;
  const ids = new Set(made.map(song => song.id));
  for (const song of made) setNativeSongNote(song.id, note).catch((error: Error) => onError(error.message));
  updateLibrarySongs(songs => songs.map(song => (ids.has(song.id) ? { ...song, note } : song)));
  return note;
}

function card(id: string, fields: CardFields): Song {
  return {
    id,
    jobId: fields.jobId,
    playlistId: fields.playlistId,
    title: fields.title,
    style: fields.style ?? '',
    lyrics: fields.lyrics ?? '',
    coverUrl: '',
    duration: '--:--',
    createdAt: fields.createdAt ?? new Date(),
    isGenerating: true,
    stage: 'stageWaitingInQueue',
    tags: ['yue2'],
  };
}

/** The tracks a job makes: its songs times the variations of each. */
function tracksOf(job: YueJob): number {
  const of = (key: string) => Math.max(1, Number(job.generation_settings?.[key]) || 1);
  return of('lm_batch_size') * of('synth_batch_size');
}

/** The rows a job's cards are drawn in, top to bottom: one a track, the first of them the card the request began as. */
export function rowsFor(first: string, tracks: number): string[] {
  return Array.from({ length: tracks }, (_, index) => (index === 0 ? first : `${first}~${index + 1}`));
}

/** The row a song took: `made` are its job's songs in the library, newest first, and the first song a job finished
 *  took its lowest card, the last one the card on top. */
export function rowTaken(rows: string[], made: Song[], song: Song): string | undefined {
  return rows[rows.length - (made.length - made.indexOf(song))];
}

/** The cards still waiting for a song: a job keeps a card for each track the library does not hold yet, its lower
 *  cards leaving first, so a finished song takes the row of the card under which nothing moves. */
export function stillWaiting(cards: Song[], madeOf: ReadonlyMap<string, Song[]>, rowsOfJob: ReadonlyMap<string, string[]>): Song[] {
  const kept = new Map<string, number>();
  return cards.filter(entry => {
    if (!entry.jobId) return true;
    const left = (rowsOfJob.get(entry.jobId)?.length ?? 1) - (madeOf.get(entry.jobId)?.length ?? 0);
    const seen = kept.get(entry.jobId) ?? 0;
    kept.set(entry.jobId, seen + 1);
    return seen < left;
  });
}

/** `made` with the songs `known` held before and the library lists no longer, each job's newest first: a song deleted
 *  while its job still runs keeps its place among the job's songs, so the job's cards and rows do not shift. */
export function keepingDeleted(made: Map<string, Song[]>, known: ReadonlyMap<string, Song[]>): Map<string, Song[]> {
  for (const [job, before] of known) {
    const there = made.get(job) ?? [];
    const gone = before.filter(song => !there.some(entry => entry.id === song.id));
    if (gone.length > 0) made.set(job, [...there, ...gone].sort((a, b) => (a.id < b.id ? 1 : -1)));
  }
  return made;
}

/** Asks the engine to stop a job; a refusal is told, the card is already marked. */
async function stopJob(jobId: string): Promise<void> {
  const response = await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}`, { method: 'POST' });
  if (!response.ok) throw new Error(`The engine did not stop the job (${response.status})`);
}

/** "Without stopping" sends the same request again: one that failed would fail on every round. */
function generationFailed(): void {
  window.dispatchEvent(new CustomEvent('yue:generation-failed'));
}

/** Removes a stopped or failed job for good; a job still running is left (409). */
async function removeEnded(jobId: string): Promise<void> {
  try {
    const response = await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}`, { method: 'DELETE' });
    // 404: the service no longer has it, which is what removing asked for
    if (!response.ok && response.status !== 404) throw new Error(`The song could not be removed: HTTP ${response.status}`);
  } finally {
    void queryClient.invalidateQueries({ queryKey: endedJobsKey });
  }
}

export function useGenerations({ enabled, notify, onFinished }: GenerationOptions) {
  const { t, language } = useI18n();
  const [cards, setCards] = useState<Song[]>([]);
  // job -> the rows its cards were drawn in, top to bottom; kept after the cards are gone, so the
  // songs that took the rows keep them
  const [rowsOfJob, setRowsOfJob] = useState<ReadonlyMap<string, string[]>>(() => new Map());
  const library = useLibrarySongs().data ?? NO_SONGS;
  const settling = useRef(new Set<string>());
  // jobs this window stopped: a read of the running jobs sent before the stop
  // landed still lists them, and they must not come back as cards
  const stopped = useRef(new Set<string>());
  const cardsNow = useRef(cards);
  useEffect(() => {
    cardsNow.current = cards;
  }, [cards]);

  // the songs each job made, newest first as the library lists them. A song deleted while this window is open keeps
  // its place among them: a job's cards and rows count every song it handed over, not only the ones still there.
  const handed = useRef(new Map<string, Song[]>());
  const madeOf = useMemo(() => {
    const made = new Map<string, Song[]>();
    for (const song of library) {
      if (!song.madeByJob) continue;
      const songs = made.get(song.madeByJob);
      if (songs) songs.push(song);
      else made.set(song.madeByJob, [song]);
    }
    handed.current = keepingDeleted(made, handed.current);
    return handed.current;
  }, [library]);

  // a card stays until the library holds its song, so its row is never empty
  const waiting = useMemo(() => stillWaiting(cards, madeOf, rowsOfJob), [cards, madeOf, rowsOfJob]);
  const following = waiting.filter(entry => entry.jobId && entry.isGenerating).length;

  const noteError = useEffectEvent((message: string) => notify(`${t('songNote')}: ${message}`, 'error'));
  // The service files each take as it imports it, so the library usually
  // shows a job's first song while the job still reads as running: its card
  // leaves the list then, and is finished from here once the job's answer is read.
  const leftEarly = useRef(new Map<string, Song>());
  const finishedJobs = useRef(new Set<string>());
  useEffect(() => {
    if (waiting.length === cards.length) return;
    const staying = new Set(waiting.flatMap(entry => (entry.jobId ? [entry.jobId] : [])));
    for (const entry of cards) {
      // the last card of a job that still reads as running
      if (entry.jobId && entry.isGenerating && !staying.has(entry.jobId) && !finishedJobs.current.has(entry.jobId)) leftEarly.current.set(entry.jobId, entry);
    }
    setCards(prev => stillWaiting(prev, madeOf, rowsOfJob));
  }, [waiting, cards, madeOf, rowsOfJob]);

  // the cards take the service's time for their job: a window on another
  // computer has another clock, and the cards are ordered by this time. A job
  // of several tracks gets a card for each, under the one the request began as.
  const follow = useCallback((cardId: string, job: YueJob) => {
    const rows = rowsFor(cardId, tracksOf(job));
    setCards(prev => prev.flatMap(entry => (entry.id === cardId ? rows.map(id => ({ ...entry, id, jobId: job.id, createdAt: new Date(job.submitted_at) })) : [entry])));
    setRowsOfJob(prev => (prev.has(job.id) ? prev : new Map(prev).set(job.id, rows)));
  }, []);

  const remove = useCallback((cardId: string) => {
    setCards(prev => prev.filter(entry => entry.id !== cardId));
  }, []);

  /** Takes every card of a job away, or the one card of a request that has no job yet. */
  const removeCards = useCallback((of: Song) => {
    setCards(prev => prev.filter(entry => (of.jobId ? entry.jobId !== of.jobId : entry.id !== of.id)));
  }, []);

  // One read of the running jobs for all cards, while any is running. It goes
  // on in a hidden window: "without stopping" sends the next song from there.
  const jobs = useQuery({
    queryKey: activeJobsKey,
    queryFn: () => readJson<YueJob[]>('/v1/music/jobs'),
    enabled,
    refetchInterval: query => (following > 0 || (query.state.data?.length ?? 0) > 0 ? POLL_MS : false),
    refetchIntervalInBackground: true,
  });

  // A job of several songs hands each one over as it is finished: it joins the library, and its playlist, while the
  // rest are still made.
  const takenEarly = useRef(new Set<string>());
  const takeEarly = useEffectEvent((running: YueJob[]) => {
    for (const job of running) {
      if (job.status !== 'running') continue;
      const made = (job.songs ?? []).filter(entry => !takenEarly.current.has(entry.song.id)).map(entry => mapNativeLibrarySong(entry.song));
      if (made.length === 0) continue;
      made.forEach(song => takenEarly.current.add(song.id));
      updateLibrarySongs(songs => [...made.filter(song => !songs.some(entry => entry.id === song.id)), ...songs]);
      if (job.playlist_id) {
        const into = job.playlist_id;
        updateLibraryPlaylists(lists => lists.map(list => (list.id === into
          ? { ...list, songIds: [...(list.songIds ?? []), ...made.map(song => song.id).filter(id => !(list.songIds ?? []).includes(id))] }
          : list)));
        playlistsChanged();
      }
      notify(t('trackReady'), 'success');
    }
  });
  useEffect(() => {
    if (jobs.data) takeEarly(jobs.data);
  }, [jobs.data]);

  // Jobs that ended without a song stay as cards, across restarts, until removed.
  const ended = useQuery({
    queryKey: endedJobsKey,
    queryFn: () => readJson<YueJob[]>('/v1/music/jobs/ended'),
    enabled,
    staleTime: Infinity,
  });
  const endedCards = useMemo(() => {
    const shown = new Set(cards.flatMap(entry => (entry.jobId ? [entry.jobId] : [])));
    return (ended.data ?? []).filter(job => !shown.has(job.id)).map(job => ({
      ...card(`${ENDED_PREFIX}${job.id}`, {
        title: job.title || t('generating'),
        style: job.style,
        lyrics: job.lyrics,
        createdAt: new Date(job.submitted_at),
        playlistId: job.playlist_id,
      }),
      isGenerating: false,
      stage: job.status === 'failed' ? 'failed' : 'cancelled',
      failure: songErrorText(job, language),
      failureCode: job.code,
    }));
  }, [ended.data, cards, t, language]);

  const finish = useEffectEvent((finished: Song, job: YueJob) => {
    finishedJobs.current.add(job.id);
    // newest first, as the library lists them, so the first takes the row
    let made = (job.songs?.length ? job.songs : job.song ? [job.song] : [])
      .map(entry => mapNativeLibrarySong(entry.song))
      .sort((a, b) => (a.id < b.id ? 1 : -1));
    if (made.length === 0) {
      removeCards(finished);
      generationFailed();
      notify(songErrorText(job, language) || t('generationFailed'), 'error');
      return;
    }
    const note = takePendingNote(finished.id, made, noteError);
    if (note) made = made.map(song => ({ ...song, note }));
    // a song the job handed over earlier and the person has deleted since is not brought back
    const deleted = new Set((handed.current.get(job.id) ?? []).filter(song => !library.some(entry => entry.id === song.id)).map(song => song.id));
    updateLibrarySongs(songs => [...made.filter(song => !deleted.has(song.id) && !songs.some(entry => entry.id === song.id)), ...songs]);
    if (job.playlist_id) {
      // the songs are in their playlist at once, so a list showing it keeps their row
      const into = job.playlist_id;
      updateLibraryPlaylists(lists => lists.map(list => (list.id === into
        ? { ...list, songIds: [...(list.songIds ?? []), ...made.map(song => song.id).filter(id => !(list.songIds ?? []).includes(id))] }
        : list)));
      playlistsChanged();
    }
    onFinished(finished.id, made);
    notify(made.length > 1 ? `${made.length} ${t('tracksReady')}` : t('trackReady'), 'success');
  });

  const settle = useEffectEvent(async (running: Song) => {
    const jobId = running.jobId;
    if (!jobId || settling.current.has(jobId) || finishedJobs.current.has(jobId)) return;
    settling.current.add(jobId);
    try {
      const job = await readJson<YueJob>(`/v1/music/jobs/${encodeURIComponent(jobId)}`);
      if (job.status === 'completed') {
        leftEarly.current.delete(jobId);
        finish(running, job);
      } else if (job.status === 'failed' || job.status === 'cancelled') {
        leftEarly.current.delete(jobId);
        // the cards leave with the running jobs and one comes back from the ended ones
        removeCards(running);
        void queryClient.invalidateQueries({ queryKey: endedJobsKey });
        if (job.status === 'failed') generationFailed();
        notify(songErrorText(job, language) || t('generationFailed'), job.status === 'failed' ? 'error' : 'info');
      }
      // still queued or running: the list was read before this job was sent
      // an ended song can bring the hub's next step, and the hub's asks wait for no song to run
      if (job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled') hubStateChanged();
    } catch (error) {
      leftEarly.current.delete(jobId);
      removeCards(running);
      notify(error instanceof Error ? error.message : String(error), 'error');
    } finally {
      settling.current.delete(jobId);
    }
  });

  const reconcile = useEffectEvent((listed: YueJob[]) => {
    const running = new Set(listed.map(job => job.id));
    // an agent's jobs, and the ones sent before this window was opened
    const shown = new Set([...cards, ...leftEarly.current.values()].flatMap(entry => (entry.jobId ? [entry.id, entry.jobId] : [entry.id])));
    const fresh = listed.filter(job => !shown.has(job.id) && !(job.client_ref && shown.has(job.client_ref)) && !stopped.current.has(job.id));
    if (fresh.length > 0) {
      setCards(prev => [
        ...fresh.flatMap(job => rowsFor(`restored_${job.id}`, tracksOf(job)).map(id => card(id, {
          title: job.title || t('generating'),
          style: job.style,
          lyrics: job.lyrics,
          jobId: job.id,
          createdAt: new Date(job.submitted_at),
          playlistId: job.playlist_id,
        }))),
        ...prev,
      ]);
      setRowsOfJob(prev => {
        const next = new Map(prev);
        for (const job of fresh) next.set(job.id, rowsFor(`restored_${job.id}`, tracksOf(job)));
        return next;
      });
    }
    const short = new Set(listed.filter(job => job.card_memory_short).map(job => job.id));
    if (cards.some(entry => entry.jobId && short.has(entry.jobId) && !entry.memoryShort)) {
      setCards(prev => prev.map(entry => (entry.jobId && short.has(entry.jobId) && !entry.memoryShort ? { ...entry, memoryShort: true } : entry)));
    }
    // a job that left the list finished, failed or was stopped elsewhere
    for (const entry of [...waiting, ...leftEarly.current.values()]) {
      if (entry.jobId && entry.isGenerating && !running.has(entry.jobId)) void settle(entry);
    }
  });

  useEffect(() => {
    if (jobs.data) reconcile(jobs.data);
  }, [jobs.data, jobs.dataUpdatedAt]);

  // The engine renders one job at a time in the order they came, so the
  // oldest running card is the one its progress belongs to.
  const making = following > 0;
  const stageClock = useRef<StageClock | null>(null);
  useEffect(() => {
    if (!making) return;
    return followEngineProgress(progress => {
      if (!progress || progress.stage === 'transcribe') return;
      const stage = STAGE_LABEL[progress.stage];
      const detail = stageDetail(progress.detail, progress.stage, stageClock);
      setCards(prev => {
        const running = prev.filter(entry => entry.isGenerating && entry.jobId);
        if (running.length === 0) return prev;
        // of a job's cards the lowest one shows the work: the song being made takes that card's row
        const active = running.reduce((oldest, entry) => (entry.createdAt <= oldest.createdAt ? entry : oldest));
        return prev.map(entry => {
          if (!entry.isGenerating || !entry.jobId) return entry;
          if (entry.id === active.id) {
            return entry.progress === progress.fraction && entry.stage === stage && entry.stageDetail === detail ? entry : { ...entry, progress: progress.fraction, stage, stageDetail: detail };
          }
          return !entry.progress && entry.stage === 'stageWaitingInQueue' ? entry : { ...entry, progress: 0, stage: 'stageWaitingInQueue', stageDetail: undefined };
        });
      });
    });
  }, [making]);

  const generate = useCallback(async (request: YueRequest) => {
    const id = `temp_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`;
    setCards(prev => [card(id, { title: request.title?.trim() || t('generating'), style: request.style, lyrics: request.lyrics, playlistId: request.playlist_id }), ...prev]);
    try {
      const response = await fetch('/v1/music/jobs', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...request, client_ref: id }),
      });
      const job = (await response.json().catch(() => null)) as (YueJob & { error?: string }) | null;
      if (!response.ok || !job || job.status === 'failed') {
        throw new Error((job && songErrorText(job, language)) || job?.error || `The engine rejected this request (${response.status})`);
      }
      follow(id, job);
      if (job.tracks_asked) notify(t('tracksForCardHint').replace('{asked}', String(job.tracks_asked)), 'info');
      if (job.laid) {
        notify(t('scoreLaidToast').replace('{seconds}', String(Math.round(job.laid.seconds))), 'info');
        const crowded = job.laid.crowded;
        if (crowded) notify(crowded.ratio > 1 ? t('scoreCrowdedNotes').replace('{ratio}', crowded.ratio.toFixed(1)) : t('scoreCrowdedWords'), 'info');
      }
    } catch (error) {
      remove(id);
      generationFailed();
      notify(error instanceof Error ? error.message : t('generationFailed'), 'error');
    }
  }, [follow, remove, notify, t, language]);

  /** A job sent from elsewhere in this window (a re-render) gets its card. */
  const track = useCallback((jobId: string, fields: CardFields) => {
    const id = `replay_${jobId}`;
    setCards(prev => (prev.some(entry => entry.jobId === jobId)
      ? prev.map(entry => (entry.jobId === jobId ? { ...entry, title: fields.title, style: fields.style ?? entry.style, lyrics: fields.lyrics ?? entry.lyrics } : entry))
      : [card(id, { ...fields, jobId }), ...prev]));
    setRowsOfJob(prev => (prev.has(jobId) ? prev : new Map(prev).set(jobId, [id])));
  }, []);

  const cancel = useCallback(async (jobId: string) => {
    stopped.current.add(jobId);
    // a stopped job of several tracks is one stopped card: the tracks it finished are songs already
    setCards(prev => {
      let marked = false;
      return prev.flatMap(entry => {
        if (entry.jobId !== jobId) return [entry];
        if (marked) return [];
        marked = true;
        return [{ ...entry, isGenerating: false, stage: 'cancelled' }];
      });
    });
    try {
      await stopJob(jobId);
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), 'error');
    }
  }, [notify]);

  /** Drops a cancelled card, stopping its job if it still runs. */
  const reset = useCallback(async (key: string) => {
    if (key.startsWith(ENDED_PREFIX)) {
      await removeEnded(key.slice(ENDED_PREFIX.length)).catch(error => notify(error instanceof Error ? error.message : String(error), 'error'));
      return;
    }
    const target = cardsNow.current.find(entry => entry.jobId === key || entry.id === key);
    if (!target) return;
    if (target.jobId) stopped.current.add(target.jobId);
    removeCards(target);
    if (target.jobId && target.isGenerating) {
      try {
        await stopJob(target.jobId);
      } catch (error) {
        notify(error instanceof Error ? error.message : String(error), 'error');
      }
    }
    if (target.jobId) await removeEnded(target.jobId).catch(error => notify(error instanceof Error ? error.message : String(error), 'error'));
  }, [removeCards, notify]);

  /** A song whose score could not be written made again with its LoRA at 1.0 on Composition (the service keeps the
   * request); the failed card goes once the new song is on its way. */
  const scoreAgain = useCallback(async (key: string, how: 'score-strength' | 'again' = 'score-strength') => {
    const jobId = key.startsWith(ENDED_PREFIX) ? key.slice(ENDED_PREFIX.length) : key;
    try {
      const response = await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}/${how}`, { method: 'POST' });
      const body = (await response.json().catch(() => null)) as (YueJob & { error?: string }) | null;
      if (!response.ok || body?.status === 'failed') throw new Error((body && (body.error || songErrorText(body, language))) || `HTTP ${response.status}`);
      await removeEnded(jobId);
      void queryClient.invalidateQueries({ queryKey: activeJobsKey });
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), 'error');
    }
  }, [notify, language]);

  /**
   * Stops the songs this window is making. With `everything` it stops every
   * job of the service: another window's, an agent's, ones sent before this
   * window was opened.
   */
  const cancelAll = useCallback(async (everything = false) => {
    // "without stopping" would send the form again the moment the queue empties
    window.dispatchEvent(new CustomEvent('yue:cancel-all'));
    const making = cardsNow.current.filter(entry => entry.isGenerating);
    const mine = making.flatMap(entry => (entry.jobId ? [entry.jobId] : []));
    const marks = new Set(making.map(entry => entry.id));
    mine.forEach(id => stopped.current.add(id));
    setCards(prev => prev.filter(entry => !entry.isGenerating));
    // The service's list: a request whose answer is still on its way back has
    // no job id here, but the service knows it by this window's mark.
    let listed: YueJob[] = [];
    try {
      listed = await readJson<YueJob[]>('/v1/music/jobs');
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), 'error');
    }
    const ours = listed.filter(job => everything || (job.client_ref && marks.has(job.client_ref)));
    const ids = [...new Set([...mine, ...ours.map(job => job.id)])];
    ids.forEach(id => stopped.current.add(id));
    const results = await Promise.allSettled(ids.map(stopJob));
    const refused = results.find((result): result is PromiseRejectedResult => result.status === 'rejected');
    if (refused) notify(refused.reason instanceof Error ? refused.reason.message : String(refused.reason), 'error');
    void queryClient.invalidateQueries({ queryKey: activeJobsKey });
  }, [notify]);

  // The list for the create page: the cards, then the library; a song of a
  // job is drawn in the row of the card it took the place of. The first song
  // a job finishes takes its lowest card, the last one the card on top.
  const keyed = useMemo(() => library.map(song => {
    const rows = song.madeByJob ? rowsOfJob.get(song.madeByJob) : undefined;
    const made = song.madeByJob ? madeOf.get(song.madeByJob) : undefined;
    if (!rows || !made) return song;
    const row = rowTaken(rows, made, song);
    return row ? { ...song, viewKey: row } : song;
  }), [library, madeOf, rowsOfJob]);
  const songs = useMemo(() => (waiting.length || endedCards.length ? [...waiting, ...endedCards, ...keyed] : keyed), [waiting, endedCards, keyed]);

  return {
    songs,
    activeJobCount: following,
    isGenerating: waiting.some(entry => entry.isGenerating),
    generate,
    track,
    cancel,
    reset,
    scoreAgain,
    cancelAll,
  };
}

if (typeof window !== 'undefined') {
  // an agent's job, or one sent before a reload, is read at once
  window.addEventListener('studio:jobs-changed', () => {
    void queryClient.invalidateQueries({ queryKey: activeJobsKey });
    void queryClient.invalidateQueries({ queryKey: endedJobsKey });
  });
}
