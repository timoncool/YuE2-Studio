# Changelog

What changed, newest first. Dates are release dates; the studio is versioned by its
Windows build.

## 2026-10-08 — 3.5.0

### Added

- **Extend a song.** Re-render takes an extension of 15 seconds to 2 minutes: the track keeps
  its own semantic tokens to the last frame and the model composes on from there.
- **Words follow the score** (HOT-Step's lyric schedule): with a supplied score each section's
  words stay out of the composer's sight until the score reaches them, so covers and long songs
  keep the voice with the band. On by default; a switch under Advanced.
- **Realaudio decoder: Auto, On, Off.** Auto decodes with the YuE2 checkpoint alone, as its
  authors, audio.cpp and ComfyUI render it, and merges Mothersuperior's decoder adapter only
  under a LoRA trained in the studio, which was trained over it. On and Off force it.
- **Hugging Face mirror** in Settings - Network (hf-mirror.com) for downloads where
  huggingface.co is slow or blocked, after wangsoft's fork.
- **Score as PDF.** The score view saves the notation as an A4 PDF, each system whole.
- **Engine progress on the card**: the stage, its step counter and the time left.
- **Video export settings**: frame size from 1080p to 240p and AAC from 128 to 320 kbps.
- Songs up to 10 minutes. The window keeps its size, place and maximised state; the player its
  repeat and shuffle. Quitting while a song is made asks first and stops it; a song the studio
  was closed on starts again. From [pull request #51](https://github.com/timoncool/YuE2-Studio/pull/51) by
  stalexxx, which also runs the service on macOS with Metal.
- **Instrumental from a score.** A button in the score view moves every Vocal note to the
  instrument part with its pitch and length, as m-a-p's yue2-instrumental skill does, and keeps
  the chords; the instrument keeps what it played where the voice was silent.
- **A note on every song, and its parameters** sent to the form, shown as JSON and saved to a
  file, as in ACE-Step Studio.
- **Parakeet Ultra int8** for karaoke: Moondream's fine-tune of Parakeet, quantized, a recogniser
  of its own beside v3, which stays as it was.
- Catalog: raspy rock-soul, quiet storm R&B and dark Sufi fusion LoRAs by becausereasons.
- The particle slider LoRAs of ntc-ai at their v2 release.

### Fixed

- A LoRA trained in the studio gets the style phrase it was trained on ("T, in the style of T.").
- Audio to MIDI no longer needs CUDA 13: it runs on CUDA where the engine does and on the
  processor elsewhere (Pascal, AMD, Intel); the new package is fetched once.
- The MIDI editor kept notes in place after an unknown meta event (files from Reaper); a file of
  instrument parts no longer turns one into the voice; Save MIDI of the score wrote a broken file.
- The exported video's visualizer moves as in the player; its background no longer freezes on
  zoom and pan.
- Models from a folder: the window says what was taken, copied and still missing, and the start
  screen shows why the engine did not start.
- Updating a LoRA from the catalogue left its old weights beside the new ones, and the engine
  refused the pair.
- Linux groundwork from SkySlider's fork: the graphics card is named on Linux, child processes
  end with the studio, and setup no longer claims a download that is not happening.

### Changed

- **A song is kept as the model made it.** The engine hands over its float output and the
  studio encodes it once, changing nothing on the way: lossless 24-bit FLAC by default, written
  by libFLAC 1.5.0, the reference encoder; MP3 by LAME when chosen. Generation no longer
  normalises the peak: Normalise is a stage under Process, after mastering.
- **Tags in every kept format** (lofty): title, artist, genre, tempo, lyrics and cover go into
  a FLAC's Vorbis comments and picture block as into an MP3's ID3v2.4.
- **Engine: yue2.cpp of 8 October.** SheetSage2 reads MERT-v2 from a file of its own (F32 or
  Q8_0); the new transcriber is downloaded once in Settings - Models, and the studio runs without
  it until then. Covers transcribe exactly as before.
- **Trainer: HOT-Step of 7 October**, about 14% faster per step with identical weights.

## 2026-10-01 — 3.4.0

### Added

- **Song and render controls.** Voice and tempo fields; section, line/stanza and case operations; Keep/Words lyric preservation and exact checkpoint tokens. Batch score-plan selection and repeated-section chord mirroring, MIDI grid/octave controls, −24 to +24 semitone transposition, native HT-Demucs vocals-only MP3/32-bit WAV output, and individual LoRA switches with −10 to +10 weights.

- **Structured song writing.** Compose the style from musical fields and lyrics from editable sections, or edit their plain text. The writer accepts a language, a target of 8–32 lines and additional instructions.

- Editable chord and section lanes above the MIDI piano roll; chord timing and section markers survive saving and reopening.

- **The MIDI editor.** A piano roll with tracks, instruments and drums (signal, MIT, played
  through the A320U SoundFont) opens in three places: in Studio tools on a new song, played
  in from a MIDI keyboard or the computer's keys, recorded or drawn; on a track's MIDI, kept
  on the track in place of the transcription; and on the create form's score, read in as MIDI
  and put back into the form by Apply. A new song is saved to the library as a track, its
  audio rendered through the same SoundFont and its MIDI kept beside it, and To cover opens
  the cover form on it with its MIDI read into the score.
- **A MIDI file becomes the score.** The tracks are listed with the voice and the instrument
  chosen, chords read from a Chords track or guessed, the file's karaoke words offered for
  the lyrics; a tune without sections has the lyrics laid along its phrases when the song is
  made.

### Changed

- **Training on the card with HOT-Step's recipe of 29.09:** Fast 100 x 4, Balanced 200 x 4,
  Thorough 300 x 8, the decoder on 60-second pieces, LoKr 128/4/256. Training is offered only
  where the engine runs its CUDA 13 build (driver 580 or newer); a trainer whose CUDA does not
  load stops and names the driver it needs instead of training on the processor.
- **The decoder companion** (Mothersuperior's nar_lora_joint_v9) is part of every model set:
  the engine merges it under every render and the trainer keeps it frozen under every LoRA.
  It leaves the LoRA catalogue, and a copy already in a library keeps working.

## 2026-09-30 — 3.3.0

### Added

- **A cover for every track.** A track without a cover of its own gets one chosen in
  Settings - Cover art: a free (CC0) photograph from Wikimedia Commons that fits the genres,
  moods and instruments of its style, found without a key; a pattern in one of 21 DiceBear
  styles (CC0), waves by default; or a cover generated through OpenRouter for every new track,
  the prompt templates shown in that mode. The picture is chosen by the track's seed, so it
  stays the same after a restart, and a stem wears its song's.
- **Save the cover into the track**, on by default: the photograph or pattern is stored with
  the track and written into the MP3's tags, tracks made before included, so a player shows
  it after the download; off, it is only shown in the studio. Changing the look draws the
  placeholders again, and a cover of the track's own is never touched.
- **The cover window** offers the same choices for one track: Commons photographs by search,
  starting from the scenes its style calls up, the track's pattern in any style with Another
  variant, generation through OpenRouter, or a file of one's own; a chosen photograph keeps a
  link to its page.
- **The video editor's background and centre picture** are chosen in the same window, the
  background also from Commons clips free of copyright; the Random background is a
  photograph for the track's style.
- **Activity**, the last item of the sidebar, after News: a log of every change an agent
  makes through the MCP server - a generation started, a song edited or deleted, a playlist,
  stems, karaoke, a dataset and its preparation, training, a LoRA, a model downloaded or
  removed, a setting - and of every message the studio shows, kept by the service (the latest
  500) and read by every window in its own language, with how long ago and the exact moment.
  (#34)
- **Sort** beside Filters on the Create page and beside the library's tabs: newest or oldest
  first, by title, by length, and recently liked in Liked Songs; each list keeps its own, and
  songs being made stay on top. (#34)
- **The Create list shows the playlist the songs go into**, the way a workspace does: with a
  playlist chosen in "Into playlist" the list beside the form is that playlist and its songs
  being made; "Nowhere" is the whole library. (#34)
- Song rows say when a song was made, how long ago with the exact moment on hover, and the
  details give the date with the time. (#34)

### Changed

- The hand-written pattern generator is replaced by DiceBear 11, drawing the same picture in
  the window and in the service, which writes it into the track as a PNG.
- The Pexels browser, which needed an API key, and the random photograph from picsum.photos,
  which answers Russia with 403, are gone.
- "Draw a cover for every new track" is now the Generation mode of Settings - Cover art.
- **A like is kept with the song** in the library instead of one window's storage: every
  window and the agent see the same likes (the agent without an open window), they survive a
  profile change, and Liked Songs start from the latest. Likes an earlier version kept in the
  window move into the library once. (#34)
- **A stem is shown under the song it was separated from**, behind "Stems · N" with its
  parts' icons, on the Create page and in the library: the song count no longer counts stems,
  a list plays its songs and a stem plays alone. Covers, re-renders and processed songs stay
  songs of their own. (#34)
- Separating a song again asks first, naming the stems it replaces; separating a stem is
  disabled with the reason, and the service refuses it to an agent too. (#34)
- The playlist page reads the shared library, so a deletion, an edit or a song made anywhere
  shows at once, and deleting a playlist asks in the studio's own dialog.
- Tools that write over stored content (song, playlist, dataset and LoRA edits, a dataset
  prepared again, stems, karaoke, MIDI, covers, the OpenRouter key) are marked destructive,
  so the agent's client asks before them. (#34)

### Fixed

- A playlist created, changed or deleted in one window shows in the others at once.
- An agent's song edit that leaves the metadata out keeps what the studio stores there - the
  like, processed versions, karaoke, the cover - instead of wiping it.
- An agent's command goes to the window the person turned to, not the one opened last. (#34)
- Every icon button of the player, the song card and the library rows has a name for screen
  readers, the same words as its tooltip. (#34)
- While the library is being read, the list says so instead of "no songs match the filters".
- Stem names are shown in the window's language.

## 2026-09-30 — 3.2.0

### Added

- **One page for everything the studio downloads and runs.** Settings - Models lists, besides
  the engine sets, the assistant, stems and karaoke, the training pack, the style-by-ear pack and
  Audio to MIDI with its sizes, each fetched, cancelled and removed there as where it is used. The
  optional parts are closed rows marked optional, and the ready-made sets say that one is enough.
- **What runs where**, on that page and in the README: which part of the studio uses an NVIDIA
  card, an AMD or Intel card, or the processor. The AMD and Intel paths are experimental.
- **AMD and Intel cards.** The writing assistant runs on llama.cpp's Vulkan build, and karaoke's
  Parakeet and the tempo model run on the card through DirectML. Stems separate on the processor,
  and training, which needs an NVIDIA card with CUDA, is no longer offered where it cannot run.
  (#25)
- **A new playlist from the Create form**: "+ New playlist…" in the playlist choice, which is
  shown even before there is any playlist.
- **The maximum duration is kept** between sessions. (#27)
- **"Take as they are" on the train step** too, so songs whose lyrics layout or style no
  assistant wrote can be trained without one.

### Changed

- **"Without stopping" starts by a slide**: pull its knob to the end to switch it on; once on,
  the same place is a bright button that switches it off. A stray click no longer starts it; the
  keyboard does with the right arrow, Enter or Space.
- **The library stays still when a song arrives**: the generation's card becomes the song in its
  own row, songs being made stay on top, and the row keeps its height.
- **Progress comes from the engine** as it works, pushed to the window, and a song is streamed
  with byte ranges, so it starts and seeks at once.

### Fixed

- **Play on a song that has just finished** while other songs are still being made. (#28)
- **The trainer on any processor**: the training pack carries every processor build of ggml
  beside CUDA, so the trainer and the captioner start without AVX2 or an NVIDIA card instead of
  "no backend available". An installed studio downloads the new trainer once and no longer calls
  an older one with options it does not know ("unknown option --loudness-lufs"). (#29, #30)
- **Style by ear without the training pack**: the captioner carries its own libraries, so
  describing a song by ear works on a machine that never downloaded the trainer - an AMD or Intel
  one included - on the processor there.
- **Tags and lyric files in a legacy code page** - Chinese GBK, Windows-1251 and others - are
  read as Windows reads them instead of as mojibake. (#26)
- **Settings and panels on Windows 7 and 8.1** (WebView2 before 111) are no longer transparent.
  (#26)
- **The style assistant starts from the style you wrote** and keeps its instruments and
  constraints. (#33)
- **The assistant cannot loop inside a title or a style**: short fields have a length limit, so
  a local model no longer runs to its token limit. (#32)
- Separation, karaoke timing and the tempo on an AMD card no longer try CUDA. (#25)

## 2026-09-29 — 3.1.0

### Added

- **Training the way the base model was trained.** The LoRA recipe is now HOT-Step's
  base-matched one, its default since 27 September after listening tests on a full album: AdamW
  with the YuE2 report's settings, several songs per step, the whole song through the sound half,
  no KL stop and no lyric timing, so no vocal separation either. Three sizes - Fast (50 steps of
  4 songs), Balanced (100 of 4, the default) and Thorough (200 of 8). The previous recipe stays as
  "As before: stop by KL", and a run started in 3.0 trains further the way it started. The
  training guide says the same in all five languages.
- **LoRA for ComfyUI.** A trained LoRA is saved as one file for ComfyUI's native YuE2 - both
  halves in one, every module a plain LoRA, so the stock loader and the YuE2 LoRA node take it
  whole. On the LoRA page and as `lora_export_comfyui` for agents.
- **The studio from another computer** (Settings - Appearance - Access from the network, off by
  default). The service listens on the network and hands a browser the studio itself; the access
  key is shown in the settings and asked once. A tunnel or proxy on the same computer needs the
  key too. (#22)
- **The card to compute on.** With two or more NVIDIA cards, Settings - Engine picks the one the
  engine, the trainer and the assistant run on. (#19)
- **Stop after this track**, a fourth position of the repeat button. (#17)
- **Player buttons in the sidebar** can be hidden: Winamp, equalizer, visualiser. (#19)
- **Without stopping and bigger queues.** The Create form takes more than ten songs at once, and
  "Without stopping" keeps making songs from the form as it was when turned on. New songs can go
  straight into a playlist.
- **Listen to the score** and download it as MIDI, played on the studio's own voices.
- **Stems of any library track**, imported ones included, from the song's panel.
- **A cover's style by ear**: "Describe by ear" hears the source track with MOSS-Music and writes
  the style from it.
- **A key for your own assistant server** (Settings - Assistant). (#24)
- **Seven more LoRA in the catalogue**: militant reggae, chanson, qawwali, Bulgarian voices, folk
  troubadour, canzone italiana and melodic death metal, each with its trigger.
- **A switch for the stock photo** of a track without a cover (Settings - Cover art), on by
  default as before; off, the track shows its drawn pattern and nothing is fetched. (#16)
- AIFF and Apple Lossless (ALAC) files are read wherever audio is taken.

### Fixed

- **The library keeps up**: stems, processed versions, a song an agent made and a song
  deleted in another window show up at once, without reopening the page.
- **Cancel all stops everything**: the song still on its way to the engine too, and "Without
  stopping" switches off instead of starting the next one.
- Stem separation starts on Auto: the card when its runtime is installed, the processor
  otherwise, instead of showing the card chosen and quietly running on the processor.
- A playlist names each song's artist as the player does.
- **Preparation without an assistant** no longer fails every song: it says an assistant writes
  the lyrics layout and the style, with "Set up the assistant" and "Take as they are". (#23)
- **The writing wand** explains why it opens the assistant settings: music models do not write
  lyrics, and the built-in assistant is one button away. (#18)
- **"yue-server returned 404: job not found"** now says why the engine restarted - out of video
  memory, a CUDA error - instead of the bare code.
- **The assistant on GTX 900 and 10-series cards and older drivers**: llama.cpp's CUDA 12 build
  where CUDA 13 does not run. (#19)
- **WebView2 that will not install** stops the installer with a plain message and Microsoft's
  standalone installer link.
- **A slow training run says why**: it shows what it computes on, and warns when that is the
  processor or when the card's memory is full.
- The training card could fail while a run was going; playlist durations showed NaN; a suggested
  cloud model never filled in; a song title and creator answered clicks with an error.
- An assistant answer no longer carries the model's reasoning into a style or lyrics.
- Engine calls ride out a dropped local connection instead of failing the song.
- MOSS-Music describes the whole track, not its intro.
- The licence text matches YuE2's: individual creators may sell the songs they make. (#21)

### Updated

- llama.cpp b11236, the yue2.cpp engine with upstream's ggml, the HOT-Step trainer at 3e7a0778.
- Tauri 2.12, React 19.3, Vite 8, Tailwind 4, TypeScript 7, vitest 5, lucide 1; Rust crates on
  their current majors (reqwest 0.13, symphonia 0.6, rusqlite 0.40, sysinfo 0.39, tower-http 0.7,
  zip 8, ort 2.0.0-rc.13).

## 2026-09-26 — 3.0.0

### Added

- **An equalizer.** Ten bands on Winamp's frequencies, -12 to +12 dB, and a preamp, with the
  curve the filters really play drawn above the sliders. The band gains are solved as Spotifast
  does, so neighbouring bands no longer pile up and a preset sounds as its sliders show. Winamp's
  eighteen presets, presets for situations (Bass Booster, Vocal Booster, Small Speakers, Night
  Listening...), the user's own, and Winamp's .EQF files in and out. Balance and mono work with
  the equalizer off too.
- **A visualiser.** MilkDrop through Butterchurn with several hundred presets, or a spectrum
  analyser in ten looks from Winamp bars to a radial one. It floats over the studio and is sized
  by its corner, goes fullscreen, or moves into a window of its own that hears the studio live.
  Next and previous, hold a preset, random or in order, as MilkDrop's keys N, P, L, R, T and F.
- **A Winamp mode.** The whole window becomes a classic Winamp 2 player: main window, equalizer,
  playlist and MilkDrop, skinned. Its windows move apart and dock as Winamp's did, the playlist
  and MilkDrop resize, and a click between them goes to whatever lies below. Winamp's own base
  skin comes with the studio, and ten more (the Classified trio and Webamp's own favourites);
  every skin is drawn sharp at any scale, resampled once to the screen's pixels. The Winamp Skin
  Museum is one button away, and a .wsz is added with a click or dropped on the player. Scale
  from 100 to 300 %, always on top, random skin, and a
  "don't ask" that starts it at once from the player bar or the sidebar. The song, its place,
  the volume and the equalizer go over to Winamp and come back to the studio; Ctrl+M switches.
- **All of it for agents.** equalizer_*, visualizer_*, winamp_* tools; library_songs_list takes
  since/until (today, yesterday, a date), library_liked and library_song_like give the user's
  best, player_play plays a list of songs as the queue, ui_press_key takes shortcuts, and
  ui_screenshot now shows MilkDrop.
- **Save as, and a Files panel.** Songs, stems, MIDI, lyric sheets, scores, requests and videos
  are saved where the user says, in Windows' own Save dialog, which starts in the folder chosen
  last - before, the browser dropped them in Downloads without a word. Each save shows in a Files
  panel, from Dub Studio: a chip in the sidebar, or a panel dragged anywhere, with its progress
  while it is written and "Show in folder" once it is.
- **A proxy for the whole studio** (Settings - Providers - Proxy): as Windows is set, the
  user's own, or none. HTTP, HTTPS, SOCKS5 and SOCKS4, with a login, written in any usual form -
  `host:port`, `host:port:login:password`, `login:password@host:port` or
  `socks5://login:password@host:port`; SOCKS5 resolves site names on the proxy. Model downloads,
  Hugging Face, OpenRouter, lyrics lookups and updates go through it and take a change at once;
  the window's own image and video search takes it at the next start. "Check" tries Hugging Face
  and OpenRouter through the proxy on the form before it is saved and says why one fails.

### Fixed

- **A news item without tags no longer blanks the window**, and **a Hugging Face error on the
  LoRA page stays in its own tab** and clears on the next attempt instead of hanging over the
  page until a restart. The repository pins the MSVC Rust toolchain, so a machine whose rustup
  defaults to GNU builds too, and the changelog script builds from a source archive with no git
  history. Thanks to @Astemiir for all four.
- **Checked in four review rounds.** Leaving Winamp always gives the window its frame and size
  back, also after a reload; floating panels keep their place through a small window; training a
  run further no longer races an install; an unreadable saved proxy is said in Settings; a failed
  save leaves no half file; the local assistant's model list says why it is empty.
- **The writing wands are always there.** With no assistant set up they were hidden, and nothing
  said the studio could write a style or lyrics at all; now they open the assistant's settings.
- **The engine is found behind a proxy.** A proxy set in HTTP_PROXY or ALL_PROXY took the
  studio's requests to its own engine on 127.0.0.1 as well, and the window waited on "Loading the
  models into memory" for good. The studio's own traffic, and the local network's, now always goes
  straight.
- **The Library page scrolls.** Everything below the first screen of it was cut off.
- **Covers keep the song's key and chords.** SheetSage2 spells keys and chords its own way
  (A#:minor, Db:maj in the key of C# minor), and the ABC score YuE2 is given took those names
  as written, so a cover could come out in the wrong key or with the wrong harmony. The score
  is now written as ComfyUI writes it since its fix of the same thing: the key named by its
  usual tonic and each chord spelled for the key it sits in. Checked against ComfyUI on every
  key and chord SheetSage2 can decode.
- **The writing assistant stops.** With Ollama the magic wand could not be stopped: the
  studio named no length, Ollama generates without end when none is named, and a small model
  that never closed the style string listed words until the app was restarted. Stop also left
  the model writing, because the service went on reading an answer nobody waited for. A local
  server is now told how long each answer may be, a run that reaches it ends with a message
  instead of a wait, and Stop closes the connection, which is what makes the model stop.
- **A local server's address saves as soon as it is typed.** The window asked every half-typed
  address for its models, one request per keystroke, each waiting out its timeout, and the save
  queued behind them for seconds: the wand could say no assistant was set up. The models are
  asked for once the address stops changing.
- **A local server's context is checked before the assistant writes.** The writing
  instructions take about 7,000 tokens, and LM Studio loads a model with 8,192 by default,
  Ollama with less: LM Studio cut the answer in seconds, and Ollama silently dropped the start
  of the instructions, so the model wrote with none and ran on. The studio now reads the
  context LM Studio or Ollama runs the model with and, when it is too short, says so at once
  with where to raise it; an answer cut short says whether the context or the length ran out.
- **Cyrillic letters in the assistant's answer** no longer turn into `�`: a letter split
  between two pieces of the stream was decoded piece by piece.
- **A song added to a dataset brings its lyrics** from its own tags (ID3 USLT, Vorbis
  LYRICS) when no text file lies beside it.

## 2026-09-26 — 2.1.2

### Fixed

- **Songs and LoRA an agent makes appear in the window at once.** A song an agent started
  over MCP only showed after a reload, and so did a LoRA it installed: the service now tells
  the window when an agent changes something, and the window reads its songs, jobs, LoRA and
  settings again. Songs the window sends itself are told apart by a mark on the request.
- **Deleting a song removes its files.** The song left the library but its audio, stems and
  cover stayed on disk: the paths were compared as written, and one side was written with
  `\\?\`.
- **Titles for songs without one.** A prose description is named after what it describes,
  and a LoRA's trigger word is never the title.
- **Song counts** take the form each language asks for: 2 песни, 13 песен, 21 песня.
- **A model download** counts only what comes down, not the files of the set already on disk.

## 2026-09-25 — 2.1.1

### Fixed

- **Create video everywhere.** The song menu of the Library page, of the song details and of
  the player had no Create video (nor Re-render, Reuse prompt or Delete in some of them): each
  place built its own menu. There is one song menu now, the same wherever it opens, and Create
  video is also a button on every track row and in the song details.
- **A menu near the bottom of a panel** opens upward, or the panel scrolls it into view,
  instead of hiding its last items under the edge.

## 2026-09-25 — 2.1.0

### Added

- **Train further.** A run that is not there yet goes on from its latest checkpoint: set
  the steps to reach and press **Train further** on the run. The same recipe and songs, the
  optimizer as it was; the loss chart and the checkpoints continue instead of starting over.
  Stops at that step. MCP: `training_continue`; `training_status` shows each run's
  `resume_step`.
- **Section tags for your own lyrics.** The tag button beside the lyrics lays them out in
  [Verse 1], [Chorus], [Bridge]... without changing a word: the assistant only says where
  each section starts, and the lines go under the tags as written. Tags already there are
  replaced. MCP: `assistant_sections`.
- **The song details show the sampling** of each stage when it was changed from the
  model's own.

### Fixed

- **Renaming a track.** The pencil in the song details and the title in the library did
  nothing for most tracks: songs of the local library carried no owner, so the studio took
  them for someone else's. Every track can be renamed from both places now, and the library
  row shows a pencil on hover.
- **Models kept in several folders.** **Use models I already have** looked only at the top
  of the folder you pick; it now searches its subfolders too (hidden ones aside), off the
  window's thread.
- **Wider side panels.** The create panel and the song details stretch up to 1200 px on a
  wide screen, never past 40% of the window; a double click on the edge puts the default
  width back, and the arrow keys move the focused edge.
- **Max tokens above 9000 did nothing.** The engine caps the audio codes at the duration
  times 25, so a longer budget never reached it and a long song stopped where the duration
  ended. The form now says how long the stage is for the duration set, and warns when max
  tokens asks for more. Songs per request and variations say what each of them does.

## 2026-09-25 — 2.0.0

### Added

- **Any track to MIDI.** A song, a stem or a processed take becomes multi-instrument MIDI -
  34 instrument groups and drums, each on its own channel - with MuScriptor (Kyutai & Mirelo)
  on the GPU through HOT-Step's native port. It is on the tools page and in every track's
  menu (**To MIDI**); a piano roll fills in while it listens, the MIDI plays against the
  original with a crossfade and per-instrument mute and solo, and the .mid is kept beside the
  track and saved from there. Nothing is installed up front: the transcriber (126 MB) and the
  chosen model - small 0.4 GB, medium 1.2 GB, large 5.5 GB - download the first time, or
  ahead from the tools page. The weights come from an open mirror of the official files, so
  no Hugging Face sign-in is needed; they are CC BY-NC 4.0, for non-commercial use. MCP:
  `midi_transcribe`, `midi_get`, `midi_status`, `midi_install`, `midi_remove`,
  `midi_delete`, `midi_cancel`, and `studio_wait until: midi`.
- **Tracks made by tools are tracks of the library.** Stems, a kept processing, a re-render and a cover of a library song
  are new tracks linked to the one they were made from: the list says **Made from «...»**
  with the tool, the click opens the original, and the track keeps the tool's settings.
  Splitting a song again replaces its stems instead of adding more.
- **An MCP server in the studio.** `http://127.0.0.1:8791/mcp` gives an agent 160 tools: every
  route of the studio's API, called inside the process, and the window itself - a
  screenshot, its controls, the player and the video editor - through a bridge the page
  answers. Files are passed by their path; the model's writing rules and official
  examples are tools, resources and prompts, so a connected agent writes instead of the
  studio's small assistant. `docs/mcp-skill.md` is the
  skill an agent reads.
- **The agent as the studio's assistant.** Pick **Agent (MCP)** as the writing assistant and
  the write buttons and a dataset preparation ask the connected agent what they would ask
  the local model, with the same instructions and answer schema. Settings has an **Agent
  (MCP)** page: whether an agent and the window are connected, the address and the lines to
  paste into Claude Code or any other client.
- **MCP 2026-07-28.** The server speaks the stateless revision (`server/discover`, per-request
  `_meta`, `Mcp-Method`/`Mcp-Name` headers checked against the body, cacheable lists,
  structured results) and the handshake revisions for older clients, and answers only this
  computer's agents and its own window. An agent reads and fills the create page's form,
  sees every control of the window with its label and the song it belongs to, shows the user
  a message and reads the window's console. `llms.txt` and the README tell an agent given
  the repository how to install, connect and start.
- **A dataset in one drop.** The training page is a three-step wizard: drop a folder of
  songs, check them, train. Albums with a cue sheet are cut into songs; titles and artists
  come from the tags, the file name and the folders.
- **Lyrics from the databases players use.** LRCLIB, then QQ Music, then Kugou, matched by
  title, artist and length, kept word for word. Only a song none of them knows has its
  vocals separated and is heard by Whisper, which is told the language the found lyrics are
  in and has its usual hallucinations (subtitle credits, captions of sounds, 674 known
  phrases in 11 languages) filtered out.
- **Sections without touching the words.** For a published sheet the assistant only says
  where each section starts; the studio puts the sheet's own lines under the tags, so no
  line can be lost or merged. Lines sung more than once are marked so the chorus stands out.
- **Every song shows where it is.** Lyrics and descriptions appear as each song is done,
  with the stage and its count on top. One model is on the card at a time, each loaded once
  for the whole batch.
- **Picks up after a restart.** Each song keeps its lyrics and style state in the dataset,
  and the job itself is kept on disk: after a crash or a restart the preparation carries on
  by itself and redoes nothing. A failed or unfinished song has a button that finishes just
  that song.
- **A trigger word from the start.** Every dataset gets a rare word made from its name
  (`nrmnkhffn` for "Нейромонах Феофан"); a word the user clears stays cleared.
- **README lists every model** the studio downloads — training, listening, lyrics, stems,
  assistant — with its direct link, size and the folder it goes in.
- **Stop by drift or by epochs.** The run stops when the composition half drifts too far
  from the base, or after a set number of passes over the songs.
- **Describing by ear on the card.** MOSS-Music listens to every song and the assistant
  writes its YuE2 style; the tempo is measured by Beat This! on the card, loaded once. The
  key is no longer measured: YuE2's style never states it and the score's key comes from
  SheetSage2.

### Fixed

- **Resampling to 16 kHz is band-limited.** Every reader of 16 kHz audio - Whisper, Parakeet,
  and now MIDI - got the audio through linear interpolation, which folds everything above
  8 kHz back into the band as noise: MuScriptor heard a clean vocal as distorted guitar and
  drums. The studio now filters before it decimates.
- `studio_wait` until idle waited for songs, the preparation and training only; it now waits
  for stems, MIDI, covers, karaoke and processing too, and refuses a `job_id` that names no
  job instead of saying "still running" forever.
- A LoRA passed to `song_create` without strengths ran at zero on every slot; it now starts
  where the create page starts it - its own strengths, else full on every slot it touches.
- A song's delete names the files it could not remove (a player holding one) instead of
  only logging them. MCP names the studio's own version.
- `video_set` and `create_form_set` refuse a field or a value the window does not have
  (`aspectRatio`, not `aspect_ratio`), instead of saying "Set" and changing nothing.
- A kept processing is named by its label; a render started by an agent can no longer leave
  the next manual export going to the studio's folder.
- A seed left to chance was drawn by the engine as a 64-bit number, more than the page's
  JavaScript numbers hold exactly, so a song made without one could not be made again; the
  studio now draws a 32-bit one.
- `writing_examples` no longer ranks official requests by words like "with" and "the".
- The assistant's JSON schema reached llama-server in a field it does not read, so local
  answers were never held to it; it now goes where llama-server reads it.
- The engine watcher no longer starts the music engine, and with it unloads the assistant,
  while a preparation or a training run holds the card.
- A song deleted during a preparation fails alone instead of stopping the job.
- A title that starts with a number keeps it ("99 Luftballons").

## 2026-09-24 — 1.1.2

### Fixed

- **Runs on every NVIDIA card from the GTX 900 series on.** A GTX 1660 Super stopped at the
  first song with "PTX was compiled with an unsupported toolchain"
  ([#2](https://github.com/timoncool/YuE2-Studio/issues/2)): the engine carried only PTX for
  Turing, which a driver older than the CUDA toolkit cannot compile. The studio now ships two
  CUDA builds of the engine with compiled code for every architecture, and picks the one the
  card and its driver run:
  - CUDA 13 for Turing and newer — GTX 16, RTX 20–50, Tesla T4, A100, RTX A-series, L4/L40,
    H100 — with driver 580 or newer;
  - CUDA 12 for Maxwell, Pascal and Volta — GTX 900/1000, Titan X/Xp/V, Tesla M40, P40, P100,
    V100 — and for any card on a driver from 525 to 579.

  The cuBLAS of that build is downloaded once, as before.
- **A 6 GB card has room for the song.** The engine reserved its cache for the model's full
  context whatever the song's length: 5.4 GB for a song under guidance, more than a 6 GB card
  holds beside the model. The cache is now sized to the song — about 1.5 GB for 130 seconds —
  and the audio comes out the same to the byte.
- **Cards before Ampere** (Maxwell, Pascal, Volta, Turing) get the engine's FP16 clamp on
  their own: their tensor cores accumulate in FP16, which can overflow into silence.

### Engine

- yue2.cpp e4f7a64: the cache sized per stage, and the CUDA backend chosen by the studio.

## 2026-09-24 — 1.1.1

### Fixed

- **The create page keeps what was typed in it.** Leaving it for the library, search or any
  other page reset it to the defaults: the style, the lyrics, the score and every setting
  were lost. The page now stays as it was left
  ([#1](https://github.com/timoncool/YuE2-Studio/issues/1)).
- **Songs play after the studio's folder moves.** The library kept each song's full path,
  so a drive that came back under another letter after a restart, or a portable folder
  copied elsewhere, left every song saying it was no longer available while the files
  sat in the media folder. Songs are now found by name in the studio's own media folder.
- **Errors say why.** Stem separation, karaoke and cover art showed only the first line
  of a failure ("load the separation model ..."), without its cause; the whole reason is
  shown now. When the graphics card cannot load the separation model, the message says
  to choose the processor instead.

### Added

- **Select everything in the LoRA catalogue** that is not downloaded yet, in one click,
  and download it as one set.

## 2026-09-24 — 1.1.0

### Added

- **LoRA.** A LoRA page with the installed files, a catalogue of ready ones (styles,
  artists, sound, sliders, with their authors credited) and a search on Hugging Face that
  downloads what you pick. In the create form each LoRA gets its own strength for the
  composition and for the sound, and its trigger word goes into the style for you. The
  engine merges LoRA and LoKr at load (yue2.cpp fork `adapters`, 7647831), honours the
  rsLoRA scale, and refuses DoRA and LoHa files by name instead of playing them wrong.
- **Training your own LoRA.** An optional tab on the LoRA page. 5–20 songs of one artist
  or style become a LoRA on your card with HOT-Step's trainer and its tuned recipe: LoKr
  64/4, Prodigy, a stop when the composition half drifts past a KL of 1.4, a checkpoint
  every 50 steps, and lyric timing from the MMS forced aligner, which now reads Cyrillic
  too. Every setting of the recipe is editable under Advanced, with the defaults one
  click away. The vocals of each song are separated first, so the aligner hears the voice.
  The assistant fills in a song's lyrics by ear, and each checkpoint goes into the LoRA
  library in one click. The trainer and its weights (about 8 GB) download only when you
  open training; it needs an RTX 30-series card or newer with 11 GB of VRAM.
- **Datasets travel between studios.** A dataset is a folder with `dataset.json` and its
  audio; import one from MiniMax Music3 Studio or show the folder to take it there.
- **Audio processing.** Noise reduction, the Spectral Lifter, a vocal naturaliser, your own
  VST3 plugins in a chain, and mastering to a reference track, in that order. Plugins are
  found in the system VST3 folders, each is set up in its own window, and they run in a
  host process of their own, so a plugin that crashes does not take the studio with it.
  Compare before and after while it plays; keep the result as a version of the track,
  next to the untouched original, or throw it away.

### Changed

- **MP3 is made by the studio.** The engine renders 32-bit float and the studio encodes
  the MP3 with LAME, so nothing is lost before the encoder.
- **Section tags keep the order they are pressed in**, and the LoRA list in the create form
  is no longer cut off by its card.

## 2026-09-24 — 1.0.4

### Fixed

- **The audio editor opens with the track.** It opened blank: the waveform library it
  runs on was left out of every build since 1.0.0, so the editor stopped on start. The
  library is back, and a test now checks that every file the editor loads is built in.

### Changed

- **Newer runtimes for the add-ons.** The assistant downloads llama.cpp b11146 (CUDA 13.4)
  instead of b9966, and karaoke and stem separation download ONNX Runtime 1.30.0
  instead of 1.24.2. Add-ons already installed keep working on the versions they have.

## 2026-09-23 — 1.0.3

### Fixed

- **Karaoke follows the song through repeated choruses.** A line used to jump to a later
  repeat of itself when the recogniser heard that one more clearly, and every line sung
  in between was squeezed into a second at the end or left hanging for half a minute.
  Lines are now placed together, in order, so each chorus keeps its own lines, a line
  nobody heard is filled in between its neighbours, and a chorus the model sang twice in
  a row shows its words as they are first sung.

### Changed

- **Covers say what they need.** The cover card now says that the words must fit the
  transcribed melody: the original lyrics, or new ones with the same syllables in every
  line and the stresses on the same notes.

## 2026-09-23 — 1.0.2

### Fixed

- **Updates install from inside the studio.** Pressing Install closed the studio and
  nothing changed: the installer was started inside the studio's own process group,
  which Windows ends together with the studio, so it died a moment after starting.
  The installer is now let go before the studio exits, and it is told the folder the
  studio lives in: started from the studio it used to miss the previous folder and put
  a second copy into the default one. Versions 1.0.0 and 1.0.1 still
  carry the old behaviour, so from them the update is downloaded once by hand; from
  1.0.2 on the studio updates itself.

## 2026-09-23 — 1.0.1

### Added

- **Listen to the recording you cover.** The cover card shows the chosen track with a
  waveform player: play, pause, click the waveform to seek. After transcription the
  studio stays in Cover mode, with the score right below.

### Changed

- **No Instrumental switch.** YuE2 is trained on songs with vocals and sings whatever it
  is given: with empty lyrics it makes words up. The switch promised something the model
  does not do, so it is gone.

### Fixed

- **Deleting a song frees its disk space.** Its audio, its six stems and its cover are
  removed with it; before, they stayed in the media folder.

## 2026-09-23 — 1.0.0

The first release of YuE2 Studio: the studio of MiniMax Music3 Studio, rebuilt around
YuE2 and [yue2.cpp](https://github.com/ServeurpersoCom/yue2.cpp) at commit `ea07706`.

### Added

- **Full songs from a style and lyrics** on YuE2-3B, up to six minutes, rendered by
  `yue-server` on the GPU. Progress follows the engine's own stages: score, audio codes,
  acoustic rendering, decoding.
- **The score as a first-class part of every song.** The ABC score comes back with each
  track, is engraved as sheet music, and can be edited and sung again. Three modes —
  melody with chords, melody only, no score — with a warning and a one-click fix when a
  melody-mode score still carries chord symbols.
- **Compose the score alone** — the planning stage without singing, in seconds.
- **Covers** — SheetSage2 transcribes an uploaded recording or a library track, melody
  only or with chords, straight into the score.
- **Exact replay** — every track keeps its request and audio codes: re-render it bit for
  bit, or with other steps, a new sound seed, several variations, another format.
- **Every engine setting** — both sampling presets in full, guidance, both seeds, songs
  per request and variations, peak normalisation, MP3 bitrate or 16/24/32-bit WAV, and the
  server's own launch options (song limit, KV cache size, VAE tiling, flash attention,
  FP16 clamping, keep-loaded).
- **Prompt files in the engine's format** — JSON or YAML, compatible with the yue2.cpp
  WebUI and `yue-synth --request`; a prompt that carries audio codes asks whether to render
  the same take or sing a new one.
- **The 110 examples** that ship with yue2.cpp, one click to load.
- **Model sets** from Serveurperso/YuE2-GGUF pinned to `64b030e`: Full native BF16,
  Quality Q8_0, Balanced Q6_K, Light Q5_K_M, each with the matching SheetSage2, or a
  custom mix role by role. The studio names the set that fits your card; every file is
  checked by SHA-256 and downloads resume.
- **A writing assistant** tuned to YuE2's style tags and lyric sections, local (Gemma via
  llama.cpp) or through OpenRouter; it can also edit the score on request.
- **CUDA, Vulkan and CPU backends** in one engine, loaded at run time. NVIDIA runs on CUDA
  (cuBLAS is fetched once, on NVIDIA only). AMD and Intel run on Vulkan, experimentally:
  the Vulkan path is verified on NVIDIA, but AMD Radeon integrated graphics gave
  unintelligible vocals and discrete AMD and Intel cards are untested. The FP16 clamp is on
  for Vulkan, which stops the engine crashing on the silence it otherwise rendered.
  Settings → Local engine chooses the device.
- **Interface in five languages** — English, Russian, Chinese, Japanese, Korean.
- **Windows installer with auto-update** and a portable archive.

### Kept from MiniMax Music3 Studio

Library, playlists, word-level karaoke (Parakeet or Whisper), six-stem separation with
HT-Demucs, cover art from prompt templates, MP3 tagging, the resource monitor.

### Removed

- Cloud music generation through OpenRouter: music is always YuE2 on your machine. Cover
  art, transcription and the assistant can still use OpenRouter.
- Everything specific to MiniMax Music3: its engine, model sets, RVQ encoder and prompting
  skill.
