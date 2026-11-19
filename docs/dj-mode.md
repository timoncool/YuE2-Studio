# YuE2 DJ mode

The studio's DJ mode is a built-in two-deck mixer using the existing song library, album and playlist collections, stem tracks, and Web Audio. No additional DJ software is needed.

## Opening it

Use the turntable icon alongside EQ, Visualizer and Winamp in the main player bar or the left sidebar (including when there is no current song). Close it with **Back to Studio** or **Escape**. The studio's regular audio player is paused while the independent decks are in use. Generated songs and the library remain available in the background.

## Browsing and loading

Use the bottom-left library pane to choose All Songs, Liked, Unassigned, Stems, a Playlist, or an Album. Search the current selection by song name/style. Drag a song onto either deck, click its **A**/**B** load button, or double-click to load A. A song with separated stems has an expand button; individual stem audio can also be loaded as its own deck track.

## Deck controls

- Each deck has independent Play/Pause, Cue, Set Cue, four hot cues, Loop In/Out, Echo, filter, three-band EQ, deck gain and live audio meters.
- A **vertical pitch fader** sits beside each turntable, like a physical DJ deck. The fader changes both tempo and pitch, with ±8% / ±16% / ±50% ranges, fine-step +/- buttons and a visible **RESET** (0%). Sync can still choose speeds outside the current fader range (global limits 0.25×–2×).
- **Tap** the tempo on a track without BPM metadata; **Sync** aligns a deck to the other deck's BPM and speed when both are known.
- The vinyl platters are styled like actual turntables, with a fixed tonearm and **YuE2 DJ** label. Each platter spins smoothly at 33⅓ RPM scaled to its deck playback rate while playing, pauses when stopped, and follows the drag angle while scratching. Scratching uses overlapping, ramped forward/reverse grains when decoded audio is ready. The transport resumes after release.
- The **equal-power crossfader** controls how much of each deck reaches the final stereo mix. Double-click the crossfader or press CENTER to return to equal mix.
- Every loaded track offers **Separate 6 stems** (HT-Demucs) or **Vocals / instrumental** (RoFormer), backed by the studio's existing native separation engine. Install the chosen model and runtime in **Settings > Models > Stems** first. Separation is an asynchronous job that can run while DJ playback continues; progress appears on the deck and the newly completed stems attach without leaving DJ mode. Existing stems are reused and confirmed before replacing.
- Separated tracks expose **ORIGINAL**, **ALL STEMS**, **ACAPELLA**, **INSTRUMENTAL**, **BASS ONLY** and **DRUMS ONLY** buttons (unavailable combinations are disabled). Every available stem also has volume, mute and solo. Presets use actual stem audio, not a basic EQ approximation.
- **RESET** controls restore pitch/tempo to 0%, low/mid/high EQ to 0 dB, echo/filter to neutral, and deck gain to unity. The faders still support double-click reset.
- **Record mix** starts capturing the post-mixer output. **Stop & save recording** opens the app's normal file-save workflow; closing DJ mode while recording also saves.

## Keyboard

- **Alt+1 / Alt+2** toggle Deck A / Deck B playback.
- **Shift-click** or right-click a hot-cue button to set its timestamp; click normally to jump.
- When the platter has keyboard focus, **Left / Right** nudge its position.

## Engineering notes

The decks use a single Web Audio context and separate media elements. Per-deck stems feed their corresponding EQ/filter/echo chain; both chains feed a bus with a compressor, the crossfader and an optional MediaRecorder output. Entering DJ mode pauses the normal YuE2 player to avoid two competing audio sources.

Unlike ordinary song-list playback, DJ scratches request decoded audio for their first use. The download/decode is on demand; its responsiveness depends on the size and format of the original track and the available RAM.

The DJ feature and one-click local builder live together on the fork's `main`. Pushes for this change use **[skip ci]** to avoid starting GitHub Actions. The local builder produces a portable Windows EXE/ZIP. New versions of the DJ UI and stem integration must be validated with the local builder and real audio; a source commit alone does not prove runtime correctness.
