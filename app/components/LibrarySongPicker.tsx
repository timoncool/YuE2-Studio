import React from 'react';
import type { Song } from '../types';
import { useLibrarySongs } from '../services/studioQueries';
import Combobox, { type ComboOption } from './kit/Combobox';
import { FieldLabel } from './kit/FieldLabel';
import { useI18n } from '../context/I18nContext';
import { formCardStrings } from '../i18n/formCards';

/** Finished songs of the library: the ones a form can take as an original, a voice or a melody. */
export function useFinishedSongs(): { songs: Song[]; loaded: boolean } {
  const { data, isSuccess } = useLibrarySongs();
  return { songs: (data ?? []).filter(song => song.audioUrl && !song.isGenerating), loaded: isSuccess };
}

/** One song of the library, found by typing a piece of its title or style. `none` adds a first choice that clears it;
 * a song that left the library stays shown as `missing`, so the form does not pretend it is still there. */
export function LibrarySongPicker({ label, value, onChange, placeholder, none, missing, showLabel = true, controlClassName }: {
  label: string;
  value: string;
  onChange: (id: string, song?: Song) => void;
  placeholder: string;
  none?: string;
  missing?: string;
  showLabel?: boolean;
  controlClassName: string;
}) {
  const { songs } = useFinishedSongs();
  const { language } = useI18n();
  const options: ComboOption[] = songs.map(song => ({ value: song.id, label: song.title || '—', hint: song.style?.slice(0, 80) }));
  if (value && !songs.some(song => song.id === value)) options.unshift({ value, label: missing ?? formCardStrings[language].missingSong, disabled: true });
  if (none) options.unshift({ value: '', label: none });
  const picker = <Combobox label={label} value={value} placeholder={placeholder} options={options}
    inputClassName={`${controlClassName} font-semibold placeholder:font-normal`} onChange={id => onChange(id, songs.find(song => song.id === id))} />;
  if (!showLabel) return picker;
  return <div className="grid">
    <FieldLabel hidden>{label}</FieldLabel>
    {picker}
  </div>;
}
