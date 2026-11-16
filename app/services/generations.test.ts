import { describe, expect, it } from 'vitest';
import type { Song } from '../types';
import { keepingDeleted, rowsFor, rowTaken, stillWaiting } from './generations';

const song = (id: string, fields: Partial<Song> = {}): Song => ({
  id,
  title: id,
  style: '',
  lyrics: '',
  coverUrl: '',
  duration: '--:--',
  createdAt: new Date(0),
  tags: [],
  ...fields,
});

describe('the rows of a job of several tracks', () => {
  it('gives a job a row a track, the first of them the card its request began as', () => {
    expect(rowsFor('temp_1', 1)).toEqual(['temp_1']);
    expect(rowsFor('temp_1', 3)).toEqual(['temp_1', 'temp_1~2', 'temp_1~3']);
  });

  it('keeps a card for each track the library does not hold yet, the lower cards leaving first', () => {
    const rows = rowsFor('temp_1', 3);
    const cards = [...rows.map(id => song(id, { jobId: 'job' })), song('temp_2'), song('other', { jobId: 'single' })];
    const rowsOfJob = new Map([['job', rows], ['single', ['other']]]);
    const waiting = (made: Song[], single: Song[] = []) => stillWaiting(cards, new Map([['job', made], ['single', single]]), rowsOfJob).map(entry => entry.id);

    expect(waiting([])).toEqual(['temp_1', 'temp_1~2', 'temp_1~3', 'temp_2', 'other']);
    expect(waiting([song('first')])).toEqual(['temp_1', 'temp_1~2', 'temp_2', 'other']);
    expect(waiting([song('second'), song('first')])).toEqual(['temp_1', 'temp_2', 'other']);
    expect(waiting([song('third'), song('second'), song('first')])).toEqual(['temp_2', 'other']);
    expect(waiting([], [song('made')])).toEqual(['temp_1', 'temp_1~2', 'temp_1~3', 'temp_2']);
  });

  it('draws each song in the row of the card it took the place of, so no row moves as songs arrive', () => {
    const rows = rowsFor('temp_1', 3);
    const [first, second, third] = [song('first'), song('second'), song('third')];

    expect(rowTaken(rows, [first], first)).toBe('temp_1~3');
    expect(rowTaken(rows, [second, first], second)).toBe('temp_1~2');
    expect(rowTaken(rows, [second, first], first)).toBe('temp_1~3');
    expect(rowTaken(rows, [third, second, first], third)).toBe('temp_1');
    expect(rowTaken(['only'], [first], first)).toBe('only');
    expect(rowTaken(['only'], [second, first], second)).toBeUndefined();
  });

  it('keeps the place of a song deleted while its job still runs, so cards and rows do not shift', () => {
    const rows = rowsFor('temp_1', 3);
    const [a, b, c] = [song('a'), song('b'), song('c')];
    const known = new Map([['job', [b, a]], ['old', [song('x')]]]);

    // the first song was deleted: the library lists the second alone, then the third arrives
    const afterDelete = keepingDeleted(new Map([['job', [b]]]), known);
    expect(afterDelete.get('job')?.map(entry => entry.id)).toEqual(['b', 'a']);
    expect(rowTaken(rows, afterDelete.get('job')!, b)).toBe('temp_1~2');
    const cards = rows.map(id => song(id, { jobId: 'job' }));
    expect(stillWaiting(cards, afterDelete, new Map([['job', rows]])).map(entry => entry.id)).toEqual(['temp_1']);

    const withThird = keepingDeleted(new Map([['job', [c, b]]]), afterDelete);
    expect(withThird.get('job')?.map(entry => entry.id)).toEqual(['c', 'b', 'a']);
    expect(rowTaken(rows, withThird.get('job')!, c)).toBe('temp_1');
    expect(stillWaiting(cards, withThird, new Map([['job', rows]]))).toEqual([]);
    // a job whose songs are all deleted is still remembered
    expect(afterDelete.get('old')?.map(entry => entry.id)).toEqual(['x']);
  });
});
