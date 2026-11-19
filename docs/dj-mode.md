# YuE2 DJ mode

The studio's DJ mode is a built-in two-deck mixer using the existing song library, album and playlist collections, stem tracks, and Web Audio. No additional DJ software is needed.

## Opening it

Use the turntable icon alongside EQ, Visualizer and Winamp in the main player bar or the left sidebar (including when there is no current song). Close it with **Back to Studio** or **Escape**. The studio's regular audio player is paused while the independent decks are in use. Generated songs and the library remain available in the background.

## Browsing and loading

Use the bottom-left library pane to choose All Songs, Liked, Unassigned, Stems, a Playlist, or an Album. Search the current selection by song name/style. Drag a song onto either deck, click its **A**/**B** load button, or double-click to load A. A song with separated stems has an expand button; individual stem audio can also be loaded as its own deck track.

## Deck controls

- Each deck has independent Play/Pause, Cue, Set Cue, four hot cues, Loop In/Out, Echo, filter, three-band EQ, deck gain and live audio meters.
- The **tempo + pitch** slider changes both together, from 0.25× to 2×. Double-click the slider or press 1.00× to reset.
- **Tap** the tempo on a track without BPM metadata; **Sync** aligns a deck to the other deck's BPM and speed when both are known.
- Drag the circular record platter to scratch. The app decodes that track when scratching is used for the first time, then plays short forward or reverse audio grains while the platter is moved. The normal transport resumes afterwards.
- The **equal-power crossfader** controls how much of each deck reaches the final stereo mix. Double-click the crossfader or press CENTER to return to equal mix.
- Tracks that already have separated stems expose a **Stem Deck** toggle. Each stem then has its own volume, mute and solo. All stem players use the deck clock and rate.
- **Record mix** starts capturing the post-mixer output. **Stop & save recording** opens the app's normal file-save workflow; closing DJ mode while recording also saves.

## Keyboard

- **Alt+1 / Alt+2** toggle Deck A / Deck B playback.
- **Shift-click** or right-click a hot-cue button to set its timestamp; click normally to jump.
- When the platter has keyboard focus, **Left / Right** nudge its position.

## Engineering notes

The decks use a single Web Audio context and separate media elements. Per-deck stems feed their corresponding EQ/filter/echo chain; both chains feed a bus with a compressor, the crossfader and an optional MediaRecorder output. Entering DJ mode pauses the normal YuE2 player to avoid two competing audio sources.

Unlike ordinary song-list playback, DJ scratches request decoded audio for their first use. The download/decode is on demand; its responsiveness depends on the size and format of the original track and the available RAM.

This feature branch was intentionally committed with **[skip ci]** and has not been compiled or tested as a portable EXE. Validate it locally before merging to `main`; merging to `main` will invoke the repository's automatic Windows build workflow.
