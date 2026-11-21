import { describe, expect, it } from 'vitest';
import { sungWords, wordMatchPercent } from './takeWordMatch';

describe('comparison with the sung lyrics', () => {
  it('ignores section cues, punctuation and case; keeps repeated words', () => {
    expect(sungWords('[Chorus]\nВернись, ВЕРНИСЬ!')).toEqual(['вернись', 'вернись']);
    expect(wordMatchPercent(sungWords('Я жду тебя'), sungWords('я тебя'))).toBe(67);
  });
  it('penalises extra, changed and out-of-order words', () => {
    const ref = sungWords('take me home');
    expect(wordMatchPercent(ref, sungWords('take me home'))).toBe(100);
    expect(wordMatchPercent(ref, sungWords('take me back home'))).toBe(67);
    expect(wordMatchPercent(ref, sungWords('take me there'))).toBe(67);
    expect(wordMatchPercent(ref, sungWords('home me take'))).toBe(33);
  });
  it('segments unspaced Japanese and does not assign a winner from missing evidence', () => {
    expect(sungWords('私は歌う').length).toBeGreaterThan(1);
    expect(wordMatchPercent([], ['voice'])).toBeNull();
    expect(wordMatchPercent(['voice'], [])).toBeNull();
  });
});
