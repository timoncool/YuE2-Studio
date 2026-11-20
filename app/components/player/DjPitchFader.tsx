import React from 'react';
import type { DjDeck } from '../../services/djMixer';
import {
  MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE, PLAYBACK_RATE_PRESETS, semitonesFromPlaybackRate,
} from '../../services/playbackTempo';

const button = 'rounded-md border border-white/10 bg-white/5 px-2 py-1 text-[10px] font-semibold text-zinc-200 transition hover:border-white/25 hover:bg-white/10';

/** The DJ player uses the very same rate and semitone scale as the normal player.
 * Keep a vertical fader for DJ use, with direct presets for 0.25× through 2×.
 */
export const DjPitchFader: React.FC<{ deck: DjDeck; side: 'A' | 'B'; color: string }> = ({ deck, side, color }) => {
  const rate = deck.state.rate;
  const percentage = (rate - 1) * 100;
  const semitones = semitonesFromPlaybackRate(rate);
  return (
    <div className="flex w-[132px] shrink-0 flex-col items-center gap-1 rounded-lg border border-zinc-700 bg-[#1b1c20] px-2 py-2">
      <span className="text-[11px] font-black tracking-wider text-zinc-200">SPEED / PITCH</span>
      <div className="w-full rounded-md border border-white/10 bg-black px-1 py-1.5 text-center font-mono tabular-nums shadow-inner">
        <div className="text-[11px] font-bold text-zinc-200">{rate.toFixed(2)}× · {Math.round(rate * 100)}%</div>
        <div className="text-base font-black" style={{ color }}>{percentage >= 0 ? '+' : ''}{percentage.toFixed(1)}%</div>
        <div className="text-[10px] text-zinc-400">{semitones >= 0 ? '+' : ''}{semitones.toFixed(1)} st</div>
        <div className="mt-0.5 border-t border-white/10 pt-1 text-sm font-bold text-white" aria-label={`Deck ${side} current tempo in BPM`}>
          {deck.state.bpm ? (deck.state.bpm * rate).toFixed(1) : '—'} <span className="text-[10px] text-zinc-500">BPM</span>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <div className="relative h-[184px] w-10 shrink-0 text-[10px] text-zinc-400" aria-hidden="true">
          <span className="absolute left-0 top-0">200%</span>
          <span className="absolute left-0 top-[57%] -translate-y-1/2">100%</span>
          <span className="absolute bottom-0 left-0">25%</span>
        </div>
        <input
          type="range"
          min={MIN_PLAYBACK_RATE}
          max={MAX_PLAYBACK_RATE}
          step="0.001"
          value={rate}
          onChange={event => deck.rate(Number(event.target.value))}
          onDoubleClick={() => deck.rate(1)}
          aria-label={`Vertical pitch fader deck ${side}`}
          aria-valuetext={`${rate.toFixed(2)} times speed; ${Math.round(rate * 100)}%; ${semitones >= 0 ? '+' : ''}${semitones.toFixed(1)} semitones`}
          className="cursor-pointer accent-slate-200"
          style={{ writingMode: 'vertical-lr', direction: 'rtl', height: 184, width: 28 }}
        />
      </div>
      <div className="flex w-full gap-1">
        <button type="button" className={`${button} flex-1`} onClick={() => deck.rate(Math.max(MIN_PLAYBACK_RATE, rate - 0.001))} aria-label="Decrease pitch">−</button>
        <button type="button" className={`${button} flex-1`} onClick={() => deck.rate(Math.min(MAX_PLAYBACK_RATE, rate + 0.001))} aria-label="Increase pitch">+</button>
      </div>
      <button type="button" className={`${button} w-full`} onClick={() => deck.rate(1)} title="Reset pitch and tempo">RESET (1×)</button>
      <div className="grid w-full grid-cols-2 gap-1" aria-label={`Deck ${side} speed presets`}>
        {PLAYBACK_RATE_PRESETS.map(preset => (
          <button
            key={preset}
            type="button"
            className={`${button} px-1 ${Math.abs(rate - preset) < 0.0005 ? 'border-sky-400 bg-sky-500/20 text-white' : ''}`}
            aria-pressed={Math.abs(rate - preset) < 0.0005}
            onClick={() => deck.rate(preset)}
          >
            {preset}×
          </button>
        ))}
      </div>
    </div>
  );
};
