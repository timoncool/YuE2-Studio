import { expect, it } from 'vitest';
import { reorderVisibleCollectionSongs } from './PlaylistEditor';

it('preserves recoverable songs when visible tracks change places', () => {
  const membership = ['one', 'in-bin', 'two', 'three'];
  const visible = ['one', 'two', 'three'];
  expect(reorderVisibleCollectionSongs(membership, visible, 'three', 0)).toEqual(['three', 'in-bin', 'one', 'two']);
  expect(reorderVisibleCollectionSongs(membership, visible, 'one', 3)).toEqual(['two', 'in-bin', 'three', 'one']);
  expect(reorderVisibleCollectionSongs(membership, visible, 'one', 1)).toBeNull();
});
