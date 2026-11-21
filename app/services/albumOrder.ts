import type { Playlist } from '../types';

/** Move one album in the shared collection list without moving ordinary playlists.
 * The returned order is used optimistically in both the Library and album picker,
 * then written to the local SQLite library.
 */
export function moveAlbum(collections: Playlist[], fromId: string, toId: string): Playlist[] {
  if (fromId === toId) return collections;
  const albums = collections.filter(item => item.kind === 'album');
  const start = albums.findIndex(item => item.id === fromId);
  const end = albums.findIndex(item => item.id === toId);
  if (start < 0 || end < 0) return collections;
  const ordered = [...albums];
  const [moved] = ordered.splice(start, 1);
  ordered.splice(end, 0, moved);
  let cursor = 0;
  return collections.map(item => item.kind === 'album' ? ordered[cursor++] : item);
}
