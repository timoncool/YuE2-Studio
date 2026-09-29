import React from 'react';
import { AudioLines, ChevronRight, Drum, Guitar, MicVocal, Music, Piano } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { Song } from '../types';

/**
 * Parts of a song - the stems a separation tool made from a base track.
 *
 * The service records the link on every part: `metadata.derived.from` names the
 * base song, `tool` says what made it and `settings.stem` says which part it is.
 * Nothing has to be guessed from titles, so both lists (the session list and the
 * library's "All songs" tab) can show and fold parts the same way.
 */

/**
 * The part a track is: the stem the separation tool recorded. Null for a song.
 *
 * The stem name comes from the separator itself (`stem.name`), never from the file name,
 * the format or the order the parts arrived in - those are guesses that break silently.
 * A record that says "stems" but carries no stem name is still a part: it counts as "other"
 * rather than quietly turning into a song of its own.
 */
export const partName = (song: Song): string | null => {
    const derived = song.derived;
    if (!derived || derived.tool !== 'stems') return null;
    const stem = derived.settings?.stem;
    return typeof stem === 'string' && stem ? stem : 'other';
};

/** The i18n key for a part's name, so it reads in the person's own language. */
const PART_KEYS: Record<string, string> = {
    drums: 'stemDrums',
    bass: 'stemBass',
    vocals: 'stemVocals',
    guitar: 'stemGuitar',
    piano: 'stemPiano',
    other: 'stemOther',
};

export type PartLabelKey = 'stemDrums' | 'stemBass' | 'stemVocals' | 'stemGuitar' | 'stemPiano' | 'stemOther';

export const partLabelKey = (song: Song): PartLabelKey | null => {
    const name = partName(song);
    if (!name) return null;
    return (PART_KEYS[name] ?? 'stemOther') as PartLabelKey;
};

/// One icon per part, so a folded song still says what is inside it.
const PART_ICONS: Record<string, React.ReactNode> = {
    // One icon per stem the separator names; anything unknown reads as "other".
    drums: <Drum size={15} />,
    bass: <AudioLines size={15} />,
    guitar: <Guitar size={15} />,
    piano: <Piano size={15} />,
    vocals: <MicVocal size={15} />,
    other: <Music size={15} />,
};

export const partIcon = (song: Song): React.ReactNode => {
    const name = partName(song);
    return (name ? PART_ICONS[name] : null) || <Music size={15} />;
};

/** True when the track is itself a separation result, not a song to separate. */
export const isPart = (song: Song): boolean => partName(song) !== null;

/** A part only counts as one when the song it came from is in the same list. */
export interface SongGroups {
    /** Songs that stand on their own, oldest order kept by the caller. */
    roots: Song[];
    /** Each song's parts, keyed by the base song's id. */
    childrenOf: Map<string, Song[]>;
}

export const splitByParent = (songs: Song[]): SongGroups => {
    const present = new Set(songs.map((song) => song.id));
    const childrenOf = new Map<string, Song[]>();
    const roots: Song[] = [];
    for (const song of songs) {
        const parent = song.derived?.from;
        if (parent && present.has(parent)) {
            const list = childrenOf.get(parent);
            if (list) list.push(song);
            else childrenOf.set(parent, [song]);
        } else {
            roots.push(song);
        }
    }
    for (const list of childrenOf.values()) {
        list.sort((a, b) => a.createdAt.getTime() - b.createdAt.getTime());
    }
    return { roots, childrenOf };
};

interface PartsToggleProps {
    parts: Song[];
    open: boolean;
    onToggle: () => void;
}

/**
 * The row that folds a song's parts away: the count says how many there are, and
 * the icons say which - bass, guitar, vocals - without opening anything.
 */
export const PartsToggle: React.FC<PartsToggleProps> = ({ parts, open, onToggle }) => {
    const { t } = useI18n();
    return (
        <button
            type="button"
            onClick={(event) => { event.stopPropagation(); onToggle(); }}
            className="tap-highlight-none flex items-center gap-2 rounded-md px-2 py-1 text-[11px] text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/5 dark:hover:text-white"
        >
            <ChevronRight size={14} className={open ? 'rotate-90 transition-transform' : 'transition-transform'} />
            <span>{t('sessionParts')}</span>
            <span>{parts.length}</span>
            {!open && (
                <span className="flex items-center gap-1.5 text-zinc-400 dark:text-zinc-500">
                    {parts.map((part) => (
                        <span key={part.id} className="flex items-center" title={partName(part) || undefined}>
                            {partIcon(part)}
                        </span>
                    ))}
                </span>
            )}
        </button>
    );
};
