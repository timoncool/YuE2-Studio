import { describe, expect, it } from 'vitest';
import type { Playlist } from '../types';
import { moveAlbum } from './albumOrder';

const a = { id: 'a', kind: 'album', name: 'Rainy days' } as Playlist;
const b = { id: 'b', kind: 'album', name: 'Swing' } as Playlist;
const c = { id: 'c', kind: 'album', name: 'Chill' } as Playlist;
const p = { id: 'p', name: 'Normal playlist' } as Playlist;

describe('album reorder in both library and Add to Album', () => {
  it('moves an album without changing the ordinary playlist order', () => {
    const initial = [a, p, b, c];
    const updated = moveAlbum(initial, 'c', 'a');
    expect(updated.map(item => item.id)).toEqual(['c', 'p', 'a', 'b']);
    expect(initial.map(item => item.id)).toEqual(['a', 'p', 'b', 'c']);
  });
  it('supports moving an album later or one slot with keyboard controls', () => {
    expect(moveAlbum([a, b, c], 'a', 'c').map(item => item.id)).toEqual(['b', 'c', 'a']);
    expect(moveAlbum([a, b, c], 'a', 'b').map(item => item.id)).toEqual(['b', 'a', 'c']);
  });
  it('does not reorder non-albums or a no-op drop', () => {
    const initial = [a, p, b];
    expect(moveAlbum(initial, 'p', 'a')).toBe(initial);
    expect(moveAlbum(initial, 'a', 'p')).toBe(initial);
    expect(moveAlbum(initial, 'a', 'a')).toBe(initial);
  });
});
