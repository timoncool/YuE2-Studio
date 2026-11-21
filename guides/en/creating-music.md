# Create and edit music

[Back to YuE2 Studio](../../README.md)

## What you can do

- **A player of its own** — a ten-band equalizer on Winamp's frequencies with its presets,
  your own and .EQF files; a MilkDrop visualiser with hundreds of presets, a spectrum in ten
  looks, a black hole with its disk of light that brightens and tilts with the song, or a flight
  through a tunnel that jumps on the loud notes, over the studio, fullscreen or in its own window; and a Winamp mode that turns the whole
  window into a skinned Winamp 2 whose windows move apart, dock and resize — the original skin
  and ten more included, sharp at any scale, the Winamp Skin Museum one click away,
  Ctrl+M to switch. The song, its place and the equalizer carry over both ways.
- **Save as, and a Files panel** — songs, stems, MIDI, lyric sheets, scores and videos are
  saved where you say, in Windows' own Save dialog, and the Files panel shows each save.
- **A proxy for the whole studio** — HTTP, HTTPS, SOCKS5 or SOCKS4, with a login: model
  downloads, Hugging Face, OpenRouter and updates go through it.
- **Models on another disk** — the setup screen and Settings show where the downloads are kept
  and how much room that disk has; one button moves the model sets, LoRAs, the stem separator,
  karaoke, the assistant's models, the trainer and Audio to MIDI to another folder, leaving a link
  so everything that shares them keeps working. Songs and datasets stay. One more button brings
  them back.
- **Full songs from a style and lyrics** — up to ten minutes, in the languages the model
  sings. In Russian YuE2 lyrics, try a combining acute accent (U+0301) after the vowel,
  for example за́мок (castle) or замо́к (lock). Listen to the resulting take to check the stress.
  On an RTX 4090 with the Q8_0 set a 3:38 song renders in about 46 seconds.
- **Songs one by one** — ask for several songs at once and each lands in the library as soon as
  it is finished, ready to play while the rest are still made; a stopped request keeps the songs
  it finished.
- **Read and edit the score** — the model writes its composition in ABC notation first; the
  studio engraves it as sheet music. Edit the notes, tempo or key and create again: the
  composition stays, the performance changes. Or switch to melody-only, or no score at all.
- **Compose the score first** — write only the score from the style and lyrics, without
  singing it (the studio's take on yue2.cpp's `yue-plan`), read and fix it, then create.
- **Covers** — SheetSage2 listens to a recording and writes its melody as a score; YuE2
  then sings that melody in your style. The words have to fit the melody: the original
  lyrics work best, new ones need the same syllables line by line with the stresses on
  the same notes, or the singing drifts off the tune — lines stretch, pause and slide into
  the wrong section. Transcription mistakes carry into the cover too. A track from your
  library can be transcribed from its menu.
- **Exact replay** — every track keeps its request and its audio codes, so it can be
  re-rendered bit for bit, or re-rendered with other steps, a new sound seed, several
  variations, or another output format, without composing again.
- **110 ready examples** — the style/lyrics/score sets that ship with yue2.cpp, covers
  included, one click to load.
- **Every engine setting** — all seven sampling knobs (temperature, top-p, top-k,
  repetition penalty and its window, minimum and maximum tokens) for the score and for the
  audio codes, flow-matching steps, guidance, both seeds, lossless FLAC or MP3. Prompts open and save as JSON or YAML in the engine's own request
  format, so they move freely between the studio, the yue2.cpp WebUI and `yue-synth`.
- **Structured song writing** — build a style from its musical fields and arrange lyrics in editable sections, while keeping the plain text editable. Give the writing assistant a language, a target of 8–32 lines and additional instructions.
- **Song and render controls** — voice and tempo fields; section, line/stanza and text-case tools; Keep/Words lyric preservation and exact checkpoint tokens; batch score-plan selection and chord mirroring in repeated sections. Edit scores with MIDI grid and octave controls. Transpose generation by −24 to +24 semitones, render vocals only through native HT-Demucs as MP3 or 32-bit WAV, and switch each LoRA on or off with weights from −10 to +10.
- **A writing assistant** — a local Gemma model or OpenRouter writes the style and lyrics
  from an idea and edits the score on request; or pick your connected agent (MCP) and it
  writes instead. Lyrics you wrote yourself get their section tags with one button, the
  words exactly as you wrote them.
- **Word-level karaoke** — enhanced LRC with a timestamp on every word, aligned by Parakeet
  or Whisper. Your lyrics are kept; only the timing is borrowed.
- **Six stems on the GPU** — drums, bass, other, vocals, guitar and piano with HT-Demucs.
- **A stem mixer** — a song's stems play together on one transport over its waveform, each with
  mute, solo, volume and pan, and a loop over a section: the drums with the bass alone, or the
  band without the voice. The stem files stay as separated.
- **Any track to MIDI** — a song, a stem or a processed take becomes multi-instrument MIDI
  (34 instrument groups and drums) with MuScriptor on the GPU, in the audio.cpp server.
  A piano roll fills in while it listens; play it against the original, mute or solo
  an instrument, save the .mid. Downloaded the first time it is used.
- **Restore a track after compression** — Apollo removes the artefacts of MP3 compression
  from a song or any audio file and gives 44.1 kHz in the source's channels, as a new track
  beside it, in the audio.cpp server.
- **A MIDI editor** — a piano roll with tracks, instruments and drums: play a song in from a
  MIDI keyboard or the computer's keys, record or draw it, edit a track's MIDI, save it to the
  library as a track and send it to a cover, or open the create form's score in it. A .mid
  file becomes the score: the voice and instrument chosen, chords read or guessed, karaoke
  words offered for the lyrics.
- **Every result is a track** — stems, a processed take, a re-render and a cover land in the
  library as tracks of their own, each marked with the one it was made from and keeping the
  settings it was made with.
- **LoRA** — LoRA and LoKr files for either half of the model, the composition or the
  sound, each with its own strength, picked in the create form. A catalogue of ready ones
  with their authors credited, and a search on Hugging Face that downloads what you pick.
- **Ruach Studio's sound in one step** — the "Ruach Studio" preset in Settings - Models puts their
  fine-tune in place of the set's model and their YuE2-VAE-RS x3 decoder in place of the VAE's
  own, and downloads what is missing; the decoder alone can be chosen on any set. It keeps the
  10-16 kHz band at the source's level and adds nothing above 20 kHz, at 4 GB of the card while it
  decodes against 2.5 GB. Six more community models - Ruach Studio's IRA and IRAE merges - are
  in the list of your own models, in Q8_0 and Q6_K.
- **Train your own LoRA** — an optional tab on the LoRA page: songs of one artist or
  style become a LoRA on your own card with HOT-Step's trainer and its tuned recipe (LoKr,
  Prodigy, lyric timing from a forced aligner that also reads Cyrillic). Every setting of
  the recipe is editable. Datasets move between this studio and MiniMax Music3 Studio as a
  folder, and each saved checkpoint goes into the LoRA library in one click.
  - **A three-step wizard** — drop a folder of songs, check them, train. Albums with a
    cue sheet are cut into songs; titles and artists come from the tags, the file name and
    the folders.
  - **Preparation on its own** — lyrics come from the lyric databases players use (LRCLIB,
    QQ Music, Kugou), word for word; only a song none of them knows has its vocals
    separated and is heard by Whisper, with its usual hallucinations filtered out. The
    assistant lays the lyrics out in sections. MOSS-Music listens to every song and the
    assistant writes its style, with the tempo measured by Beat This! on the card.
  - **Every song shows where it is** — the lyrics and styles appear as each song is done,
    one model on the card at a time, each loaded once for the whole batch. A song that
    failed or was stopped has a button that finishes just that song.
  - **Picks up after a restart** — each song keeps what it has; a preparation cut off by a
    crash or a restart carries on by itself and redoes nothing.
  - **Stop by drift or by epochs** — stop when the composition half drifts too far from
    the base, or after a set number of passes over the songs.
  - **A trigger word from the start** — every dataset gets a rare word made from its name,
    which you can change.
  - **Train further** — not there yet at 750 steps? Set 1000 and the run goes on from its
    latest checkpoint with the same recipe and songs; the loss chart and the checkpoints
    continue instead of starting over.
- **Audio processing** — noise reduction, the Spectral Lifter, a vocal naturaliser, your
  own VST3 plugins in a chain, and mastering to a reference track. Compare before and
  after while it plays, then keep the result as a version of the track or throw it away.
- **The sound as the model made it** — the engine renders 32-bit float and the studio encodes
  it once, changing nothing on the way: lossless 24-bit FLAC by default (libFLAC, the reference
  encoder), MP3 with LAME when you choose it. Normalisation is a stage under Process, after
  mastering, never part of generation.
- **Export for release** — from a song's menu: WAV in 16, 24 bits or 32-bit float, FLAC in 16 or
  24, MP3 from 128 to 320 kbps, at its own rate or 44.1–96 kHz, brought on the way to a loudness
  target: −14 LUFS for streaming, −16 for Apple Music and podcasts, −23 for broadcast, or your
  own, under a true-peak ceiling. A report shows loudness and peak before and after; the song in
  the library is untouched. The same window gives the song's MIDI - the whole score, or its lead,
  voice line or chords alone - and an Ableton Live 12 project with the song, its stems and its
  MIDI on their tracks.
- **A library of plain files** — search, playlists, cover art from prompt templates, tracks
  exported with title, lyrics and cover in their own tags (Vorbis comments in a FLAC, ID3 in an
  MP3). Interface in English, Russian,
  Chinese, Japanese, Korean and Portuguese.
- **A cover for every track** — a track without one wears a free (CC0) Wikimedia Commons
  photograph that fits the genres, moods and instruments of its style, a pattern in one of 21
  DiceBear styles, or a cover OpenRouter generates for every new track. It is chosen by the
  track's seed, so it stays the same, a stem wears its song's, and it is written into the file,
  so players show it after the download.
- **One picture window** — for a cover, and for the background and centre of a music video:
  Commons photographs by search starting from the scenes a style calls up, clips free of
  copyright for a background, the track's pattern in any style, generation through
  OpenRouter, or a file of your own. No keys, no stock-site accounts.
- **Music videos** — a visualiser video of any track in 16:9, 9:16 or 1:1, with karaoke
  lyrics, text layers and effects, rendered to MP4 on your machine.
- **Activity** — a log in the sidebar of every change a connected agent makes and every
  message the studio shows.
- **No window when an agent works, a tray when you want it** — `--headless` starts the studio with
  only a tray icon, and it stops by itself after ten idle minutes; `--stop` ends it when its
  window is hidden and nothing is at work. "Keep running in the system tray when the window is
  closed" hides the window instead of quitting, and a second launch brings up the one that runs.
- **Likes, sorting and stems in order** — a like is kept with the song for every window and
  agent, every list sorts by date, title or length, and a song's stems fold under it.
- **A song from a seed** — hum, play or write an opening, turn on "Continue the song from this
  score", and the model writes the rest of the song on from it in its key and meter, the opening
  coming back as a hook.
- **Chord bed** — a score of chords over rests written at once in the tempo, key and meter you set,
  one section per lyrics tag. For a song with words the model writes its melody over those chords,
  section by section, and the song keeps the score with that melody; an instrumental is played
  from the bed as it stands, with no planning step.
- **Chords and sections** — chord variety so the song stops looping one progression, sections that
  do not open the way the one before did, chords outside the key, and the score held to the
  lyrics' sections in their order.
- **Covers that line up** — Match sections to the score retags the lyric blocks by where the source
  recording sings them; the score view moves the voice an octave down or up, apart from the band,
  and Instrumental hands the voice's notes to the instrument and keeps the chords.
- **Extend a song** — Re-render composes on from the track's last frame by 15 seconds to 2 minutes;
  with a score, each section's words wait until the score reaches it.
- **AB2 solver** — one network evaluation a step instead of two, about twice as fast at the same
  steps; midpoint stays the default.
- **Around every song** — a note of your own on each song, its parameters sent back to the form,
  shown as JSON or saved to a file; the engine's stage, step and time left on the card while it
  renders; video export from 1080p to 240p with AAC from 128 to 320 kbps; quitting while a song is
  made asks first, and a song the studio was closed on starts again; karaoke can recognise words
  with Parakeet Ultra, Moondream's fine-tune of Parakeet.

## Compose and edit MIDI

The embedded Signal editor has multiple tracks, instruments, drums, MIDI keyboard recording, tempo changes, editable chord symbols and section markers. Save a new composition as MIDI and rendered audio in the library, or update an existing track’s MIDI. SoundFonts are bundled locally; the editor sends no analytics.

In YuE2, the same editor opens ABC scores and applies notes, chords and sections back to the create form. The visual editor replaces the older separate score editor. Use **Apply** to update the ABC score, or send a library MIDI track to cover mode.

Next: [get started](getting-started.md) · [choose models and engines](models-and-runtimes.md) ·
[browse sample tracks](showcase.md) · [get help](community.md#report-a-problem).
