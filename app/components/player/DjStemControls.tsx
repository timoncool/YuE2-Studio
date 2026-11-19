import React from 'react';
import { AudioLines, Loader2, Scissors } from 'lucide-react';
import type { DjDeck } from '../../services/djMixer';
import type { DjSeparationEngine, DjSeparationStatus } from '../../services/djSeparation';
import { stemOf } from '../../services/songStems';

const button = 'rounded border border-white/10 bg-white/5 px-2 py-1 text-[10px] font-semibold text-zinc-200 hover:border-white/25 hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-30';
const slider = 'h-1.5 w-full cursor-pointer accent-sky-400';

export const DjStemControls: React.FC<{
  deck: DjDeck;
  side: 'A' | 'B';
  separation: DjSeparationStatus | null;
  onSeparate: (engine: DjSeparationEngine) => void;
}> = ({ deck, side, separation, onSeparate }) => {
  const state = deck.state;
  if (!state.song) return null;
  const busy = Boolean(separation?.run && !separation.run.done);
  const run = separation?.run?.song_id === state.song.id ? separation.run : null;
  return (
    <div className="mt-3 rounded-lg border border-sky-400/20 bg-black/30 p-3" data-testid={'dj-stems-' + side}>
      <div className="mb-2 flex items-center gap-2 text-xs font-bold tracking-wide text-zinc-200">
        <AudioLines size={15} className="text-sky-400" /> LIVE STEM CONTROL
        <span className="ml-auto text-[10px] font-normal text-zinc-400">
          {state.stems.length ? state.stems.length + ' separated channels' : 'No separated audio loaded'}
        </span>
      </div>
      {state.stems.length > 0 && (
        <>
          <div className="mb-2 flex flex-wrap items-center gap-1.5">
            {(['original', 'all', 'vocals', 'instrumental', 'bass', 'drums'] as const).map(preset => {
              const available = preset === 'original' || preset === 'all' ||
                (preset === 'instrumental'
                  ? state.stems.some(stem => stem.label !== 'vocals')
                  : state.stems.some(stem => stem.label === preset));
              const label = { original: 'ORIGINAL', all: 'ALL STEMS', vocals: 'ACAPELLA', instrumental: 'INSTRUMENTAL', bass: 'BASS ONLY', drums: 'DRUMS ONLY' }[preset];
              return (
                <button key={preset} type="button" disabled={!available} onClick={() => deck.stemPreset(preset)}
                  className={button + (preset === 'original' && !state.stemMode ? ' border-sky-400 bg-sky-500/20' : '')}
                  title={available ? 'Mix ' + label.toLowerCase() : 'This separation does not contain that channel'}>
                  {label}
                </button>
              );
            })}
          </div>
          <label className="flex cursor-pointer items-center gap-2 text-xs font-semibold text-zinc-200">
            <input type="checkbox" className="accent-sky-400" checked={state.stemMode}
              onChange={event => deck.setStemMode(event.target.checked)} />
            <AudioLines size={13} /> PLAY SEPARATED STEMS
          </label>
          {state.stemMode && <div className="mt-2 space-y-1.5">
            {state.stems.map(stem => (
              <div key={stem.id} className="flex items-center gap-2 text-[11px]">
                <span className="w-20 truncate uppercase text-zinc-400">{stem.label}</span>
                <button type="button" className={button + (stem.mute ? ' bg-rose-500/30 text-rose-200' : '')}
                  onClick={() => deck.updateStem(stem.id, { mute: !stem.mute })} title="Mute">M</button>
                <button type="button" className={button + (stem.solo ? ' bg-sky-500/30 text-sky-200' : '')}
                  onClick={() => deck.updateStem(stem.id, { solo: !stem.solo })} title="Solo">S</button>
                <input className={slider} type="range" min="0" max="1.5" step="0.01" value={stem.level}
                  aria-label={'Deck ' + side + ' ' + stem.label + ' stem volume'}
                  onChange={event => deck.updateStem(stem.id, { level: Number(event.target.value) })} />
                <button type="button" className="w-11 shrink-0 text-right tabular-nums text-zinc-400 hover:text-white"
                  title="Reset stem volume" onClick={() => deck.updateStem(stem.id, { level: 1 })}>
                  {Math.round(stem.level * 100)}%
                </button>
              </div>
            ))}
          </div>}
        </>
      )}
      {stemOf(state.song) === null && (
        <div className="mt-2 flex flex-wrap items-center gap-2 border-t border-white/10 pt-2">
          <button type="button" className={button + ' flex items-center gap-1'} disabled={busy}
            onClick={() => onSeparate('htdemucs')}><Scissors size={13} /> Separate 6 stems</button>
          <button type="button" className={button + ' flex items-center gap-1'} disabled={busy}
            onClick={() => onSeparate('roformer')}><Scissors size={13} /> Vocals / instrumental</button>
          {run && !run.done && <span className="flex items-center gap-1 text-[11px] text-sky-300">
            <Loader2 size={13} className="animate-spin" /> Separating {Math.round(Math.min(1, Math.max(0, run.progress)) * 100)}%
          </span>}
          {run?.done && run.error && <span className="text-[11px] text-rose-300">{run.error}</span>}
        </div>
      )}
      <p className="mt-2 text-[10px] leading-4 text-zinc-500">
        Native separation runs while playback continues. Install models/runtimes in Settings &gt; Models first.
        Finished stems load onto this deck automatically.
      </p>
    </div>
  );
};
