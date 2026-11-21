/** User-visible confirmations must match the actual collection type. */
export function collectionToastKey(
  kind: 'playlist' | 'album',
  action: 'created' | 'createFailed' | 'added' | 'alreadyOne' | 'alreadyMany',
) {
  const album = kind === 'album';
  switch (action) {
    case 'created': return album ? 'albumCreated' : 'playlistCreated';
    case 'createFailed': return album ? 'failedToCreateAlbum' : 'failedToCreatePlaylist';
    case 'added': return album ? 'songsAddedToNamedAlbum' : 'songsAddedToNamedPlaylist';
    case 'alreadyOne': return album ? 'songAlreadyInAlbum' : 'songAlreadyInPlaylist';
    case 'alreadyMany': return album ? 'songsAlreadyInAlbum' : 'songsAlreadyInPlaylist';
  }
}
