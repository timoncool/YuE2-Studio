import React from 'react';
import { AudioLines, Download, Loader2, Scissors } from 'lucide-react';
import type { DjDeck } from '../../services/djMixer';
import { djEngineReady, type DjSeparationEngine, type DjSeparationStatus } from '../../services/djSeparation';
import { stemOf } from '../../services/songStems';

const button = 'rounded border border-white/10 bg-white/5 px-2 py-1 text-[10px] font-semibold text-zinc-200 hover:border-white/25 hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-30';
const slider = 'h-1.5 w-full cursor-pointer accent-sky-400';

export const DjStemControls: React.FC<{
  deck: DjDeck;
  side: 'A' | 'B';
  separation: DjSeparationStatus | null;
  installing: DjSeparationEngine | null;
  actionBusy: boolean;
  onInstall: (engine: DjSeparationEngine) => void;
  onSeparate: (engine: DjSeparationEngine) => void;
}> = ({ deck, side, separation, installing, actionBusy, onInstall, onSeparate }) => {
  const state = deck.state;
  if (!state.song) return null;
  const busy = actionBusy || Boolean(separation?.run && !separation.run.done) || installing !== null;
  const run = separation?.run?.song_id === state.song.id ? separation.run : null;
  const downloading = installing !== null && separation?.download?.scope === 'separation' ? separation.download : null;
  const progress = downloading && downloading.total_bytes > 0
    ? Math.min(100, Math.round(downloading.downloaded_bytes / downloading.total_bytes * 100)) : null;
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
          {(['htdemucs', 'roformer'] as const).map(engine => {
            const ready = djEngineReady(separation, engine);
            const name = engine === 'htdemucs' ? '6 stems' : 'Vocals / instrumental';
            const label = ready ? 'Separate ' + name : 'Install ' + name + ' engine';
            return (
              <button key={engine} type="button"
                className={button + ' flex items-center gap-1.5 px-3 py-1.5'}
                disabled={busy || !separation}
                onClick={() => ready ? onSeparate(engine) : onInstall(engine)}
                title={ready ? 'Split this song using the installed native engine' : 'Download the missing separator model and runtime from YuE2'}
              >
                {installing === engine ? <Loader2 size={14} className="animate-spin" /> : ready ? <Scissors size={14} /> : <Download size={14} />}
                {installing === engine ? 'Installing ' + name + '…' : label}
              </button>
            );
          })}
          {!separation && <span className="text-[11px] text-zinc-400">Checking separation engines…</span>}
          {run && !run.done && <span className="flex items-center gap-1 text-[11px] text-sky-300">
            <Loader2 size={13} className="animate-spin" /> Separating {Math.round(Math.min(1, Math.max(0, run.progress)) * 100)}%
          </span>}
          {run?.done && run.error && <span className="text-[11px] text-rose-300">{run.error}</span>}
        </div>
      )}
      {installing && (
        <div className="mt-2 rounded border border-sky-500/20 bg-sky-500/5 p-2 text-[11px] text-sky-200" role="status">
          <div className="flex items-center gap-2">
            <Loader2 size={13} className="animate-spin" />
            <span>Downloading {installing === 'htdemucs' ? 'six-stem' : 'vocals / instrumental'} separator model and runtime…</span>
            {progress !== null && <span className="ml-auto font-mono tabular-nums">{progress}%</span>}
          </div>
          <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-zinc-700">
            <div className="h-full rounded-full bg-sky-400 transition-[width]" style={{ width: (progress ?? 3) + '%' }} />
          </div>
          <p className="mt-1 text-[10px] text-zinc-400">Progress is for the current downloaded component. Setup may fetch several files.</p>
        </div>
      )}
      <p className="mt-2 text-[10px] leading-4 text-zinc-500">
        Missing stem engines can be installed here with one click. Separate while the decks keep playing;
        finished stems attach automatically.
      </p>
    </div>
  );
};
