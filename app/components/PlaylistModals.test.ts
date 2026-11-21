import { describe, expect, it } from 'vitest';
import { collectionsForAdding } from './PlaylistModals';
import type { Playlist } from '../types';

const collections = [
  { id: 'set', name: 'Mixtape', kind: 'playlist' },
  { id: 'record', name: 'Album I', kind: 'album' },
  { id: 'legacy', name: 'Classic playlist' },
] as Playlist[];

describe('song context-menu collection destination', () => {
  it('lists only albums when Add to Album was clicked', () => {
    expect(collectionsForAdding(collections, 'album').map(item => item.id)).toEqual(['record']);
  });
  it('lists only playlists when Add to Playlist was clicked', () => {
    expect(collectionsForAdding(collections, 'playlist').map(item => item.id)).toEqual(['set', 'legacy']);
  });
  it('preserves the existing mixed view for other collection selectors', () => {
    expect(collectionsForAdding(collections, 'all')).toHaveLength(3);
  });
});
