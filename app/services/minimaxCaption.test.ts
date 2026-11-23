import { describe, expect, it } from 'vitest';
import { joinCaption, splitCaption } from './minimaxCaption';

describe('splitCaption', () => {
  it('reads a caption with only some of the headings without doubling them', () => {
    const caption = 'Global Metadata\nBasic Attributes: bpm is 84.';
    const panes = splitCaption(caption);
    expect(panes).toEqual({ globalMetadata: 'Basic Attributes: bpm is 84.', vocalDetails: '', arrangement: '' });
    expect(joinCaption(panes.globalMetadata, panes.vocalDetails, panes.arrangement)).toBe(caption);

    const twoOfThree = splitCaption('Global Metadata\nPop.\nArrangement\nPiano, drums.');
    expect(twoOfThree).toEqual({ globalMetadata: 'Pop.', vocalDetails: '', arrangement: 'Piano, drums.' });
  });

  it('keeps all three sections and a caption without headings as they were', () => {
    const caption = 'Global Metadata\nPop.\nVocal Details\nA soft voice.\nArrangement\nPiano.';
    const panes = splitCaption(caption);
    expect(panes).toEqual({ globalMetadata: 'Pop.', vocalDetails: 'A soft voice.', arrangement: 'Piano.' });
    expect(joinCaption(panes.globalMetadata, panes.vocalDetails, panes.arrangement)).toBe(caption);
    expect(splitCaption('upbeat pop with piano')).toEqual({ globalMetadata: 'upbeat pop with piano', vocalDetails: '', arrangement: '' });
  });

  it('does not split on a heading word inside a sentence', () => {
    const panes = splitCaption('Global Metadata\nThe Arrangement grows. Vocal Details come late.');
    expect(panes.globalMetadata).toBe('The Arrangement grows. Vocal Details come late.');
  });
});
