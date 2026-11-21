import React, { useEffect, useRef, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { AudioLines, Square } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { windowAudioStrings } from '../i18n/windowAudio';

type AudioWindow = { id: string; title: string };

export function WindowAudioCapture({ disabled, onRecording }: { disabled: boolean; onRecording: (file: File) => void }) {
  const { language } = useI18n();
  const text = windowAudioStrings[language];
  const [windows, setWindows] = useState<AudioWindow[] | null>(null);
  const [selected, setSelected] = useState('');
  const [loading, setLoading] = useState(false);
  const [recording, setRecording] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState('');
  const recordingRun = useRef<AbortController | null>(null);

  useEffect(() => () => {
    if (!recordingRun.current) return;
    recordingRun.current.abort();
    void invoke('stop_window_audio').catch(() => {});
  }, []);

  if (!isTauri() || !navigator.userAgent.includes('Windows')) return null;

  const load = async () => {
    setLoading(true);
    setError('');
    try {
      setWindows(await invoke<AudioWindow[]>('audio_capture_windows'));
      setSelected('');
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  };

  const record = async () => {
    const target = windows?.find(window => window.id === selected);
    if (!target || recordingRun.current) return;
    const run = new AbortController();
    recordingRun.current = run;
    setRecording(true);
    setError('');
    try {
      const data = await invoke<ArrayBuffer>('record_window_audio', { windowId: target.id });
      if (!run.signal.aborted) onRecording(new File([data], 'window-audio.wav', { type: 'audio/wav' }));
    } catch (reason) {
      if (!run.signal.aborted) setError(String(reason));
    } finally {
      recordingRun.current = null;
      if (!run.signal.aborted) {
        setRecording(false);
        setStopping(false);
      }
    }
  };

  const stop = async () => {
    setStopping(true);
    try {
      await invoke('stop_window_audio');
    } catch (reason) {
      setError(String(reason));
      setStopping(false);
    }
  };

  const button = 'inline-flex items-center justify-center gap-1.5 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-xs font-medium disabled:opacity-50 dark:border-white/15';

  return (
    <div className="mt-2 space-y-2">
      <button type="button" className={button} disabled={disabled || loading || recording} onClick={() => void load()}>
        <AudioLines size={13} /> {text.open}
      </button>
      {windows !== null && (
        <div className="space-y-2">
          <label className="flex flex-col gap-1 text-xs">
            {text.window}
            <select className="rounded border border-zinc-200 bg-transparent p-1.5 dark:border-white/15"
              value={selected} onChange={event => setSelected(event.target.value)} disabled={disabled || recording}>
              <option value="">{text.select}</option>
              {windows.map(window => <option key={window.id} value={window.id}>{window.title}</option>)}
            </select>
          </label>
          <p className="text-[11px] text-zinc-500">{text.hint}</p>
          {recording
            ? <button type="button" className={button} onClick={() => void stop()} disabled={stopping}><Square size={12} /> {text.stop}</button>
            : <button type="button" className={button} onClick={() => void record()} disabled={!selected || disabled}>{text.record}</button>}
        </div>
      )}
      {error && <p role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</p>}
    </div>
  );
}
