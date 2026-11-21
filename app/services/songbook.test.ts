import { expect, it } from 'vitest';
import { buildSongbook, chordName, placeLyricSyllables } from './songbook';
import { chordShape } from './chordShapes';
import { estimateLyricSyllables } from './songbookSyllables';

const score = `X:1
T:Verse
M:4/4
L:1/4
K:D
V:Vocal
D E F A|
w:Hel-lo beau-ty
V:Ins
"D"D2"G"G2|`;

it('lays instrument chords over the timed vocal syllables of the existing score', () => {
  const page = buildSongbook(score, 2);
  expect(page.title).toBe('Verse');
  expect(page.key).toBe('E');
  expect(page.lines[0].map(cell => [cell.chords, cell.lyrics, cell.number])).toEqual([
    [['E'], ['Hel-'], '1'], [[], ['lo'], '2'], [['A'], ['beau-'], '3'], [[], ['ty'], '5'],
  ]);
  expect(chordName('C#m7/G#', -2)).toBe('Bm7/F♯');
  expect(chordName('F#m7b5', 2)).toBe('G♯m7b5');
});

it('marks the local key changes that determine jianpu note numbers', () => {
  const source = 'X:1\nM:4/4\nL:1/4\nK:C\nC D E G | [K:D] D E F A | [K:C] C D E G |';
  const ordinary = buildSongbook(source, 0);
  expect(ordinary.key).toBe('C');
  expect(ordinary.lines.flat().map(cell => [cell.keyChange, cell.number])).toEqual([
    [undefined, '1'], [undefined, '2'], [undefined, '3'], [undefined, '5'],
    ['D', '1'], [undefined, '2'], [undefined, '3'], [undefined, '5'],
    ['C', '1'], [undefined, '2'], [undefined, '3'], [undefined, '5'],
  ]);
  const transposed = buildSongbook(source, 2);
  expect(transposed.key).toBe('D');
  expect(transposed.lines.flat().flatMap(cell => cell.keyChange ?? [])).toEqual(['E', 'D']);
  expect(transposed.lines.flat().map(cell => cell.number)).toEqual(ordinary.lines.flat().map(cell => cell.number));
});

it('places estimated plain lyrics only when every sung note has one syllable', () => {
  const page = buildSongbook(score.replace('w:Hel-lo beau-ty\n', ''), 0);
  expect(placeLyricSyllables(page.lines, estimateLyricSyllables('短歌词', 'zh'))).toBe(false);
  expect(page.lines.flat().every(cell => !cell.lyrics.length)).toBe(true);
  expect(placeLyricSyllables(page.lines, estimateLyricSyllables('[Verse]\n春天来了', 'zh'))).toBe(true);
  expect(page.lines.flat().map(cell => cell.lyrics[0])).toEqual(['春', '天', '来', '了']);
  expect(placeLyricSyllables(page.lines, ['a', 'b', 'c', 'd'])).toBe(false);
});

it('uses language-specific splitting while leaving Japanese readings of kanji unaligned', () => {
  expect(estimateLyricSyllables('beautiful', 'en')).toEqual(['beau-', 'ti-', 'ful']);
  expect(estimateLyricSyllables('природа', 'ru')).toEqual(['при-', 'ро-', 'да']);
  expect(estimateLyricSyllables('caminho', 'pt')).toEqual(['ca-', 'mi-', 'nho']);
  expect(estimateLyricSyllables('학교', 'ko')).toEqual(['학', '교']);
  expect(estimateLyricSyllables('きょう', 'ja')).toEqual(['きょ', 'う']);
  expect(estimateLyricSyllables('今日', 'ja')).toBeNull();
});

it('produces instrument-specific shapes with the requested chord and bass', () => {
  const guitar = chordShape('C/E', 'guitar');
  const ukulele = chordShape('C', 'ukulele');
  expect(guitar).toHaveLength(6);
  expect(ukulele).toHaveLength(4);
  const played = [40, 45, 50, 55, 59, 64].flatMap((open, index) => guitar![index] < 0 ? [] : [open + guitar![index]]);
  expect(Math.min(...played) % 12).toBe(4);
  expect([...new Set(played.map(note => note % 12))].sort()).toEqual([0, 4, 7]);
  expect(chordShape('C(no3)', 'guitar')).toBeNull();
});


it('uses the named vocal line before the first pitched voice when the words are separate', () => {
  const source = 'X:1\nM:4/4\nL:1/4\nK:C\nV:Ins\n"C"C, C, C, C,|\nV:Vocal\nG A B c|';
  for (const shift of [0, 2]) {
    const song = buildSongbook(source.replaceAll('\n', '\r\n'), shift);
    expect(placeLyricSyllables(song.lines, ['one', 'two', 'three', 'four'])).toBe(true);
    expect(song.lines.flat().map(cell => [cell.number, cell.lyrics[0]])).toEqual([
      ['5', 'one'], ['6', 'two'], ['7', 'three'], ['1\u0307', 'four'],
    ]);
    expect(song.lines[0][0].chords).toEqual([shift === 0 ? 'C' : 'D']);
  }
});
