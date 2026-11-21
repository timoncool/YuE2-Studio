import { describe, expect, it } from 'vitest';
import { sungWords, wordMatchPercent } from './takeWordMatch';

describe('comparison with the sung lyrics', () => {
  it('ignores section cues, punctuation and case; keeps repeated words', async () => {
    expect(sungWords('[Chorus]\nВернись, ВЕРНИСЬ!')).toEqual(['вернись', 'вернись']);
    expect(await wordMatchPercent(sungWords('Я жду тебя'), sungWords('я тебя'))).toBe(67);
  });
  it('penalises extra, changed and out-of-order words', async () => {
    const ref = sungWords('take me home');
    expect(await wordMatchPercent(ref, sungWords('take me home'))).toBe(100);
    expect(await wordMatchPercent(ref, sungWords('take me back home'))).toBe(67);
    expect(await wordMatchPercent(ref, sungWords('take me there'))).toBe(67);
    expect(await wordMatchPercent(ref, sungWords('home me take'))).toBe(33);
  });
  it('scores long contrasting lyrics and yields when the musician changes the selected pair', async () => {
    const left = Array<string>(2100).fill('sing');
    const right = Array<string>(2100).fill('again');
    expect(await wordMatchPercent(left, left)).toBe(100);
    expect(await wordMatchPercent(left, right)).toBe(0);
    expect(await wordMatchPercent(left, right, () => true)).toBeNull();
  });
  it('segments unspaced Japanese and does not assign a winner from missing evidence', async () => {
    expect(sungWords('私は歌う').length).toBeGreaterThan(1);
    expect(await wordMatchPercent([], ['voice'])).toBeNull();
    expect(await wordMatchPercent(['voice'], [])).toBeNull();
  });
});
