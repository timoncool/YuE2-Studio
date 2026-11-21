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
  studio?: string;
  space: { total_bytes: number; required_bytes: number; available_bytes: number | null };
  songs: LegacySong[];
  playlists: { id: string; name: string; song_ids: string[] }[];
}

export interface LegacySource { studio: string; path: string; version: string }
const categories = [
  ['audio', 'legacyLibraryAudio'],
  ['lyrics', 'legacyLibraryLyrics'], ['generation', 'legacyLibraryGeneration'],
  ['covers', 'legacyLibraryCovers'], ['karaoke', 'legacyLibraryKaraoke'],
  ['takes', 'legacyLibraryTakes'], ['stems', 'stems'],
  ['midi', 'legacyLibraryMidi'], ['latents', 'legacyLibraryLatents'],
  ['playlists', 'playlists'],
] as const;
type Category = typeof categories[number][0];
interface ImportIssue { title: string; file: string; code: string; error?: string }
export function LegacyLibraryPreview({ sources = [] }: { sources?: LegacySource[] }) {
  const { t, songCount, language } = useI18n();
  function formatSize(bytes: number) {
    const units = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte'] as const;
    const i = Math.min(units.length - 1, Math.floor(Math.log10(Math.max(1, bytes)) / 3));
    return new Intl.NumberFormat(language, { style: 'unit', unit: units[i], maximumFractionDigits: 2 }).format(bytes / 1000 ** i);
  }
  const [preview, setPreview] = useState<Preview | null>(null);
  const [sourceName, setSourceName] = useState('');
  const [busy, setBusy] = useState(false);
  const [copying, setCopying] = useState(false);
  const [summary, setSummary] = useState('');
  const [error, setError] = useState('');
  const [exclude, setExclude] = useState<Category[]>([]);
  const [issues, setIssues] = useState<ImportIssue[]>([]);
  async function chooseFolder(path?: string, registeredName?: string) {
    setBusy(true);
    setError('');
    try {
      const response = await fetch('/v1/library/migration/preview', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(path ? { path } : { pick: true }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || response.statusText);
      if (!result.cancelled) {
        const library = result as Preview;
        const folder = library.path.split(/[\\/]/).filter(Boolean);
        const folderName = folder.at(-1)?.toLowerCase() === 'data' ? folder.at(-2) : folder.at(-1);
        setSourceName(registeredName || library.studio || folderName || library.path);
        setPreview(library);
        setSummary('');
        setIssues([]);
        setExclude([]);
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
    setIssues([]);
    try {
      const response = await fetch('/v1/library/migration/import', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: preview.path, exclude }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || response.statusText);
      setSummary(t('legacyLibraryResult').replace('{songs}', String(result.added)).replace('{existing}', String(result.already_imported))
        .replace('{lists}', String(result.playlists_added)).replace('{knownLists}', String(result.playlists_already_imported))
        + (result.updated ? ' ' + t('legacyLibraryUpdated').replace('{songs}', String(result.updated)) : ''));
      setIssues(result.errors ?? []);
      libraryChanged();
      playlistsChanged();
      // Refresh the free-space figure after the actual copy. The result of the copy
      // stays visible even if the source folder is disconnected immediately after.
      setPreview({ ...preview, space: { ...preview.space, required_bytes: 0, available_bytes: null } });
      try {
        const refreshed = await fetch('/v1/library/migration/preview', {
          method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: preview.path }),
        });
        if (refreshed.ok) setPreview(await refreshed.json());
      } catch { /* The import already completed. */ }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setCopying(false);
    }
  }

  return <section className="mb-4 rounded-2xl border border-zinc-300 p-4 dark:border-white/15">
    <h3 className="text-sm font-semibold">{t('legacyLibraryTitle')}</h3>
    {sources.length > 0 && <ul className="mt-2 space-y-1">{sources.map(source =>
      <li key={source.path}><button type="button" disabled={busy || copying} onClick={() => void chooseFolder(source.path, source.studio)}
        className="max-w-full rounded-lg border border-zinc-300 px-3 py-2 text-left text-sm dark:border-white/20">
        <strong>{source.studio} {source.version}</strong><span className="ml-2 break-all text-xs">{source.path}</span>
      </button></li>)}</ul>}
    <button type="button" disabled={busy || copying} onClick={() => void chooseFolder()}
      className="mt-2 inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-sm disabled:opacity-60 dark:border-white/20">
      {busy ? <Loader2 size={15} className="animate-spin" /> : <FolderOpen size={15} />}
      {t(busy ? 'legacyLibraryOpening' : 'legacyLibraryChoose')}
    </button>
    {error && <p role="alert" className="mt-2 text-sm text-rose-600 dark:text-rose-300">{error}</p>}
    {preview && <div className="mt-3 space-y-2 text-sm">
      <p role="status" className="break-all"><strong>{sourceName}</strong> · {preview.path} · {songCount(preview.songs.length)} · {t('playlists')} ({preview.playlists.length})</p>
      <p className="text-xs">{exclude.length === 0 && t('legacyLibrarySize').replace('{size}', formatSize(preview.space.total_bytes)).replace('{required}', formatSize(preview.space.required_bytes))}
        {preview.space.available_bytes !== null && <> · {t('legacyLibraryFree').replace('{size}', formatSize(preview.space.available_bytes))}</>}
      </p>
      {exclude.length === 0 && preview.space.available_bytes !== null && preview.space.required_bytes > preview.space.available_bytes &&
        <p role="alert" className="text-amber-700 dark:text-amber-300">{t('legacyLibraryNoSpace')}</p>}
      <fieldset className="flex flex-wrap gap-x-4 gap-y-1" disabled={busy || copying}>
        <legend className="mb-1 font-medium">{t('legacyLibraryOptional')}</legend>
        {categories.map(([category, label]) => <label key={category} className="inline-flex items-center gap-1.5">
          <input type="checkbox" checked={!exclude.includes(category) && !(category === 'takes' && exclude.includes('audio'))}
            disabled={category === 'takes' && exclude.includes('audio')} onChange={event => {
            const checked = event.currentTarget.checked;
            setExclude(previous => checked ? previous.filter(item => item !== category) : [...previous, category]);
          }} />
          {t(label)}
        </label>)}
      </fieldset>
      <button type="button" disabled={busy || copying || (exclude.length === 0 && preview.space.available_bytes !== null && preview.space.required_bytes > preview.space.available_bytes) || (!preview.songs.length && (!preview.playlists.length || exclude.includes('playlists')))} onClick={() => void importLibrary()}
        className="inline-flex items-center gap-2 rounded-lg bg-orange-600 px-3 py-2 font-semibold text-white disabled:opacity-50">
        {copying && <Loader2 size={15} className="animate-spin" />}
        {t(copying ? 'legacyLibraryCopying' : 'legacyLibraryCopy')}
      </button>
      {summary && <p role="status">{summary}</p>}
      {issues.length > 0 && <div role="alert" className="text-rose-700 dark:text-rose-300">
        <p className="font-medium">{t('legacyLibraryImportIssues')}</p>
        <ul className="max-h-48 list-disc space-y-1 overflow-auto pl-5">
          {issues.map((issue, index) => <li key={index} className="break-all">
            {issue.code !== 'playlist_incomplete' && <>{issue.title} · </>}
            <strong>{issue.file}</strong> · {issue.code === 'missing' ? t('legacyLibraryMissing')
              : issue.code === 'playlist_incomplete' ? t('legacyLibraryPlaylistIncomplete')
              : issue.error || t('legacyLibraryImportIssues')}
          </li>)}
        </ul>
      </div>}
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
