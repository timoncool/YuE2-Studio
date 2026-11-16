import React, { useEffect, useState } from 'react';
import { Download, Music2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import { apiUrl } from '../services/apiBase';
import { saveFile } from '../services/saveFile';

interface ScoreFile { part: string; notes: number; chords: number; file_name: string }
interface Listing {
  score: { bpm: number; meter: string; key: string; files: ScoreFile[] } | { error: string } | null;
  own: { origin: 'transcribed' | 'edited'; file_name: string } | null;
}

/**
 * A song's MIDI, each file named for where its notes come from: the score the
 * song was generated with, or the track's own MIDI (heard in the audio, or
 * edited). A song made without a score has no score MIDI.
 */
export const SongMidiFiles: React.FC<{ songId: string }> = ({ songId }) => {
  const { t } = useI18n();
  const [listing, setListing] = useState<Listing | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setListing(null);
    setError(null);
    void fetch(apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/midi/parts`))
      .then(response => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
      .then(answer => { if (current) setListing(answer as Listing); })
      .catch(reason => { if (current) setError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { current = false; };
  }, [songId]);

  const save = (part: string, name: string) => void saveFile(name, { url: `/v1/library/songs/${encodeURIComponent(songId)}/midi/parts/${part}` });
  const counted = (file: ScoreFile) => [
    file.notes ? t('exportMidiNotes').replace('{n}', String(file.notes)) : '',
    file.chords ? t('exportMidiChords').replace('{n}', String(file.chords)) : '',
  ].filter(Boolean).join(', ');
  const row = (key: string, label: string, detail: string, onSave: () => void) => (
    <li key={key} className="flex items-center gap-2 py-1">
      <span className="min-w-0 flex-1"><span className="font-semibold text-zinc-800 dark:text-zinc-100">{label}</span> <span className="text-zinc-500">{detail}</span></span>
      <button type="button" onClick={onSave} className="inline-flex shrink-0 items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 font-semibold text-zinc-700 hover:border-pink-300 dark:border-white/10 dark:text-zinc-200">
        <Download size={12} />.mid
      </button>
    </li>
  );

  const score = listing?.score && 'files' in listing.score ? listing.score : null;
  const unread = listing?.score && 'error' in listing.score ? listing.score.error : null;
  return (
    <section className="mt-5 border-t border-zinc-200 pt-4 text-xs dark:border-white/10" data-testid="song-midi">
      <h3 className="flex items-center gap-1.5 text-sm font-bold text-zinc-900 dark:text-white"><Music2 size={14} />MIDI</h3>
      {error && <p className="mt-2 text-rose-600 dark:text-rose-300">{t('exportMidiFailed').replace('{error}', error)}</p>}
      {listing && (
        <>
          {score ? (
            <>
              <p className="mt-1.5 leading-5 text-zinc-500">
                {t('exportMidiFromScore').replace('{bpm}', String(score.bpm)).replace('{meter}', score.meter).replace('{key}', score.key)}
              </p>
              <ul className="mt-1">{score.files.map(file => row(file.part, t(`exportMidiPart_${file.part}` as TranslationKey), counted(file), () => save(file.part, file.file_name)))}</ul>
            </>
          ) : (
            <p className="mt-1.5 leading-5 text-zinc-500">{unread ? t('exportMidiBadScore').replace('{error}', unread) : t('exportMidiNoScore')}</p>
          )}
          {listing.own ? (
            <ul className="mt-2 border-t border-zinc-100 pt-2 dark:border-white/5">
              {row('own', t(listing.own.origin === 'edited' ? 'exportMidiOwnEdited' : 'exportMidiOwnHeard'),
                listing.own.origin === 'edited' ? '' : t('exportMidiOwnHeardHint'),
                () => save('own', listing.own!.file_name))}
            </ul>
          ) : (
            <p className="mt-2 leading-5 text-zinc-500">{t('exportMidiNoOwn')}</p>
          )}
        </>
      )}
    </section>
  );
};
