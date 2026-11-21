import { describe, expect, it } from 'vitest';
import { collectionSongAvailability, reorderVisibleCollectionSongs } from './PlaylistEditor';
import type { Song, Playlist } from '../types';
import type { TrashedSong } from '../services/nativeLibrary';

const song = (id: string): Song => ({
  id,
  title: id,
  lyrics: '',
  style: '',
  coverUrl: '',
  duration: '1:00',
  createdAt: new Date('2026-11-20T00:00:00Z'),
  tags: [],
});
const binEntry = (id: string): TrashedSong => ({
  song: song(id),
  trashedAt: new Date('2026-11-19T00:00:00Z'),
  goneAt: new Date('2026-12-19T00:00:00Z'),
  with: 0,
});

describe('collection song availability', () => {
  it.each(['album', 'playlist'] as const)('reconciles %s membership with active and Bin tracks', kind => {
    const album: Playlist = { id: 'a', name: 'Rainy Days', kind, songIds: ['one', 'two', 'three', 'four', 'five'] };
    const active = [song('one')];
    const bin = ['two', 'three', 'four', 'five'].map(binEntry);
    const result = collectionSongAvailability(album, active, bin);
    expect(result.active.map(item => item.id)).toEqual(['one']);
    expect(result.inBin.map(item => item.song.id)).toEqual(['two', 'three', 'four', 'five']);
    expect(result.missingIds).toEqual([]);
    expect(album.songIds).toEqual(['one', 'two', 'three', 'four', 'five']);
  });

  it('keeps Bin membership intact until a restored song returns to active list', () => {
    const album: Playlist = { id: 'a', name: 'Rainy Days', kind: 'album', songIds: ['one', 'two'] };
    const first = collectionSongAvailability(album, [song('one')], [binEntry('two')]);
    const restored = collectionSongAvailability(album, [song('one'), song('two')], []);
    expect(first.inBin).toHaveLength(1);
    expect(restored.active.map(item => item.id)).toEqual(['one', 'two']);
    expect(restored.inBin).toHaveLength(0);
  });

  it('reorders visible tracks without dropping hidden Bin members', () => {
    expect(reorderVisibleCollectionSongs(
      ['one', 'in-bin', 'two', 'three'], ['one', 'two', 'three'], 'three', 0,
    )).toEqual(['three', 'in-bin', 'one', 'two']);
    expect(reorderVisibleCollectionSongs(
      ['one', 'in-bin', 'two'], ['one', 'two'], 'one', 1,
    )).toBeNull();
  });

  it('separately flags memberships absent from both lists without removing them', () => {
    const album: Playlist = { id: 'a', name: 'Rainy Days', kind: 'album', songIds: ['one', 'not-found'] };
    const result = collectionSongAvailability(album, [song('one')], []);
    expect(result.active).toHaveLength(1);
    expect(result.missingIds).toEqual(['not-found']);
    expect(album.songIds).toEqual(['one', 'not-found']);
  });
});
