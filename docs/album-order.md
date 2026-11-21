# Ordering albums in YuE2 Studio

Albums can be rearranged independently of ordinary playlists. In **Library > Albums**, drag a card onto another album to move it to that position. You can also use the left/right buttons that appear when hovering or focusing an album card (useful with a keyboard). The hovered target is outlined while dragging.

The **Add to Album** picker uses the exact same order and also supports dragging its album rows. Reordering in either place updates both automatically.

## Persistence and compatibility

- The local music library stores order in a separate SQLite `album_order` table, created automatically for both new and existing portable installations. The original `playlists` and `playlist_songs` tables are unchanged.
- `PUT /v1/library/albums/order` accepts `{ "ids": ["album-id-1", "album-id-2"] }`, checks that the request contains each current album exactly once, saves it atomically, and returns the updated collection list. A stale or malformed order is rejected rather than silently losing albums.
- Album type detection supports the original description prefix and the newer artwork/collection JSON wrapper.
- `GET /v1/library/playlists` returns the saved ranking; the Library grid and Add to Album picker both filter this same order. Previously unranked collections continue using their existing newest-first fallback.
- Deleting an album removes its ordering record automatically through the database's foreign key. Reordering never edits song audio, album membership, cover art or playlist track order.

## Buttons and messages

**New album** now uses the same filled button styling and plus icon as **New Playlist**. The create-album action in the picker uses the same plus symbol.

Adding to an album now says **Added to album ...** (and uses album-specific messages when the song was already there); creating an album also confirms **Album created**, while playlist confirmations remain unchanged. Messages are translated into the six existing UI languages.

The change is committed with `[skip ci]` to avoid using GitHub Actions. Build and test through `BUILD-YUE2-LOCAL.bat` locally.
