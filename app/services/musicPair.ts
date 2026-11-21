import { useCallback, useEffect, useRef, useState } from 'react';
import type { YueRequest } from '../types';
import { SELF_READ } from './studioQueries';
import { poll } from './poll';

export type PairSource = NonNullable<YueRequest['music_source']>;
export type PairStage = 'idle' | 'waiting_for_generation' | 'checking_files' | 'needs_install' | 'unloading' | 'starting_engine' | 'loading_model' | 'ready' | 'error';

export interface PairStatus {
  instance_id?: string;
  revision: number;
  requested: PairSource | null;
  active: PairSource | null;
  stage: PairStage;
  elapsed_ms: number;
  waiting_job_id?: string | null;
  missing_bytes?: number | null;
  error?: string | null;
  load_on_create?: boolean;
}

export const pairKey = (source: PairSource | null | undefined) =>
  source ? JSON.stringify([source.kind, 'engine' in source ? source.engine : null, 'model' in source ? source.model : null]) : '';
const busyStages: PairStage[] = ['waiting_for_generation', 'checking_files', 'unloading', 'starting_engine', 'loading_model'];

const endpoint = '/v1/music/pair';

export function useMusicPair(source: PairSource | undefined, enabled: boolean, serviceInstance?: string) {
  const [status, setStatus] = useState<PairStatus>();
  const [intent, setIntent] = useState(0);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string>();
  const sent = useRef(0);
  const client = useRef('');
  const requestCount = useRef(0);
  const latest = useRef<{ status: PairStatus; request: number } | undefined>(undefined);
  const retired = useRef(new Set<string>());
  const observe = useCallback((next: PairStatus, request: number) => {
    if (!Number.isInteger(next.revision)) return;
    const previous = latest.current;
    if (previous) {
      const old = previous.status;
      if (next.instance_id === old.instance_id) {
        if (next.revision < old.revision) return;
        if (next.revision === old.revision && (request < previous.request ||
            (!busyStages.includes(old.stage) && busyStages.includes(next.stage)))) return;
      } else {
        if (!next.instance_id || retired.current.has(next.instance_id) || request < previous.request) return;
        if (old.instance_id) retired.current.add(old.instance_id);
        setSending(false);
        setError(undefined);
      }
    }
    latest.current = { status: next, request };
    setStatus(next);
  }, []);
  const key = pairKey(source);
  const busy = Boolean(status && busyStages.includes(status.stage));
  const matching = Boolean(key && key === pairKey(status?.requested));

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    const read = async () => {
      const request = ++requestCount.current;
      try {
        const reply = await fetch(endpoint, { headers: SELF_READ, signal: controller.signal });
        if (!reply.ok) return;
        const next: PairStatus = await reply.json();
        if (!controller.signal.aborted) observe(next, request);
      } catch { /* Keep the latest state when a status read is interrupted. */ }
    };
    const stop = busy ? poll(read, 400) : undefined;
    if (!stop) void read();
    return () => { controller.abort(); stop?.(); };
  }, [enabled, busy, serviceInstance, observe]);

  useEffect(() => {
    if (!enabled || !source || intent === sent.current) return;
    sent.current = intent;
    client.current ||= globalThis.crypto?.randomUUID?.() ?? `studio-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    const sequence = intent;
    const request = ++requestCount.current;
    const instanceAtSend = latest.current?.status.instance_id;
    const sameRequest = () => sent.current === sequence && (!instanceAtSend || instanceAtSend === latest.current?.status.instance_id);
    setSending(true);
    setError(undefined);
    void fetch(endpoint, { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ music_source: source, client_id: client.current, sequence }),
    }).then(async reply => {
      if (reply.status === 404 || reply.status === 405) return;
      const next = await reply.json();
      if (sent.current !== sequence) return;
      if (!reply.ok) throw new Error(next.error || `HTTP ${reply.status}`);
      observe(next, request);
    }).catch((problem: unknown) => {
      if (sameRequest()) setError(String(problem));
    }).finally(() => {
      if (sameRequest()) setSending(false);
    });
  }, [enabled, source, intent, observe]);

  return { status, error, matching, sending, busy,
    blocked: sending || Boolean(error) || Boolean(matching && status && status.stage !== 'ready' && status.stage !== 'idle'),
    select: () => { setError(undefined); setIntent(value => value + 1); },
  };
}
