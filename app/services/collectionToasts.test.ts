import { describe, expect, it } from 'vitest';
import { en } from '../i18n/en';
import { collectionToastKey } from './collectionToasts';

describe('album and playlist confirmation messages', () => {
  it('says album when adding to an album, including duplicate songs', () => {
    const added = en[collectionToastKey('album', 'added')].replace('{name}', 'Swing').replace('{count}', '1');
    expect(added).toContain('album');
    expect(added).toContain('Swing');
    expect(en[collectionToastKey('album', 'alreadyOne')]).toContain('album');
    expect(en[collectionToastKey('album', 'alreadyMany')]).toContain('album');
  });
  it('says album when creating one and still uses playlist messages for playlists', () => {
    expect(en[collectionToastKey('album', 'created')]).toContain('Album');
    expect(en[collectionToastKey('album', 'createFailed')]).toContain('album');
    expect(collectionToastKey('playlist', 'created')).toBe('playlistCreated');
    expect(collectionToastKey('playlist', 'added')).toBe('songsAddedToNamedPlaylist');
    expect(collectionToastKey('playlist', 'alreadyOne')).toBe('songAlreadyInPlaylist');
  });
});
