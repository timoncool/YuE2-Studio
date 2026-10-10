import abcjs from 'abcjs';
import { describe, expect, it } from 'vitest';
import { cueIndex, scoreCues, scoreNotes } from '../services/scorePlayback';

function engrave(abc: string): abcjs.TuneObject {
  const host = document.createElement('div');
  document.body.appendChild(host);
  return abcjs.renderAbc(host, abc, { add_classes: true })[0];
}

describe('score cursor', () => {
  it.each([
    ['common time', 'X:1\nM:4/4\nL:1/4\nQ:1/4=100\nK:C\nCDEF|GABc|c2 B2|C4|'],
    ['compound time', 'X:1\nM:6/8\nL:1/8\nQ:3/8=60\nK:C\nCDE FGA|c3 B3|'],
    ['two voices', 'X:1\nM:4/4\nL:1/4\nQ:1/4=90\nK:C\nV:Vocal\nC2 E2|G4|\nV:Ins\nC,4|G,4|'],
  ])('marks each moment when the synth plays it (%s)', (_, abc) => {
    const score = engrave(abc);
    const starts = [...new Set(scoreNotes(score).map(note => Math.round(note.start * 100)))].sort((a, b) => a - b);
    const cues = [...new Set(scoreCues(score).map(cue => Math.round(cue.at * 100)))].sort((a, b) => a - b);
    expect(cues).toEqual(starts);
  });

  it('finds the moment sounding at a time', () => {
    const cues = [{ at: 0 }, { at: 0.6 }, { at: 1.2 }];
    expect(cueIndex(cues, -1)).toBe(-1);
    expect(cueIndex(cues, 0)).toBe(0);
    expect(cueIndex(cues, 0.55)).toBe(0);
    expect(cueIndex(cues, 0.6)).toBe(1);
    expect(cueIndex(cues, 9)).toBe(2);
  });
});
