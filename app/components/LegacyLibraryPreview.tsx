import React, { useState } from 'react';
import { FolderOpen, Loader2 } from 'lucide-react';
import { libraryChanged, playlistsChanged } from '../services/studioQueries';
import { useI18n } from '../context/I18nContext';

interface LegacySong {
  id: string;
  title: string;
  files: { kind: string; name: string }[];
  missing: { kind: string; name: string }[];
}

interface Preview {
  path: string;
  songs: LegacySong[];
  playlists: { id: string; name: string; song_ids: string[] }[];
}

export function LegacyLibraryPreview() {
  const { t, songCount } = useI18n();
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [copying, setCopying] = useState(false);
  const [summary, setSummary] = useState('');
  const [error, setError] = useState('');
  async function chooseFolder() {
    setBusy(true);
    setError('');
    try {
      const response = await fetch('/v1/library/migration/preview', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ pick: true }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || response.statusText);
      if (!result.cancelled) {
        setPreview(result);
        setSummary('');
      }
    } catch (reason) {
      setError(String(reason instanceof Error ? reason.message : reason));
    } finally {
      setBusy(false);
    }
  }

  async function importLibrary() {
    if (!preview || copying) return;
    setCopying(true);
    setSummary('');
    setError('');
    try {
      const response = await fetch('/v1/library/migration/import', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: preview.path }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || response.statusText);
      setSummary(t('legacyLibraryResult').replace('{songs}', String(result.added)).replace('{existing}', String(result.already_imported))
        .replace('{lists}', String(result.playlists_added)).replace('{knownLists}', String(result.playlists_already_imported)));
      libraryChanged();
      playlistsChanged();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setCopying(false);
    }
  }

  return <section className="mb-4 rounded-2xl border border-zinc-300 p-4 dark:border-white/15">
    <h3 className="text-sm font-semibold">{t('legacyLibraryTitle')}</h3>
    <button type="button" disabled={busy || copying} onClick={() => void chooseFolder()}
      className="mt-2 inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-sm disabled:opacity-60 dark:border-white/20">
      {busy ? <Loader2 size={15} className="animate-spin" /> : <FolderOpen size={15} />}
      {t(busy ? 'legacyLibraryOpening' : 'legacyLibraryChoose')}
    </button>
    {error && <p role="alert" className="mt-2 text-sm text-rose-600 dark:text-rose-300">{error}</p>}
    {preview && <div className="mt-3 space-y-2 text-sm">
      <p role="status" className="break-all">{preview.path} · {songCount(preview.songs.length)} · {t('playlists')} ({preview.playlists.length})</p>
      <button type="button" disabled={busy || copying || (!preview.songs.length && !preview.playlists.length)} onClick={() => void importLibrary()}
        className="inline-flex items-center gap-2 rounded-lg bg-orange-600 px-3 py-2 font-semibold text-white disabled:opacity-50">
        {copying && <Loader2 size={15} className="animate-spin" />}
        {t(copying ? 'legacyLibraryCopying' : 'legacyLibraryCopy')}
      </button>
      {summary && <p role="status">{summary}</p>}
      <ul className="max-h-64 space-y-1 overflow-auto">
        {preview.songs.map(song => <li key={song.id}><details>
          <summary className="cursor-pointer">{song.title} · {t('legacyLibraryAvailable')} ({song.files.length})
            {song.missing.length > 0 && <span className="ml-2 text-amber-700 dark:text-amber-300">{t('legacyLibraryMissing')} ({song.missing.length})</span>}
          </summary>
          <ul className="ml-5 break-all text-xs">
            {song.files.map(file => <li key={file.kind + file.name}>{file.name}</li>)}
            {song.missing.map(file => <li key={file.kind + file.name}>{t('legacyLibraryMissing')} · {file.name}</li>)}
          </ul>
        </details></li>)}
      </ul>
      {preview.playlists.length > 0 && <details>
        <summary className="cursor-pointer">{t('playlists')} ({preview.playlists.length})</summary>
        <ul className="ml-5">{preview.playlists.map(list => <li key={list.id}>{list.name} · {songCount(list.song_ids.length)}</li>)}</ul>
      </details>}
    </div>}
  </section>;
}
