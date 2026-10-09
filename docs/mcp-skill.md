---
name: yue2-studio
description: Drive YuE2 Studio on this computer through its MCP server - write and make songs with YuE2 (style, lyrics, scores, covers of recordings), manage the library, draw covers, split stems, turn any track into MIDI, time karaoke, process audio, make video clips, play songs, install LoRA, and build a LoRA from a folder of songs end to end, writing the lyrics layout and styles yourself instead of the studio's small assistant. Sees and works the studio's window like a user. Use whenever the user asks for anything the studio does.
---

# YuE2 Studio through MCP

YuE2 Studio serves MCP at `http://127.0.0.1:8791/mcp` while it is open (Streamable HTTP,
stateless JSON-RPC). Every tool runs the same code as a button of the studio, and the user
sees what you do in the studio's window.

## If the studio is not running yet

1. It is a Windows desktop application. If it is not installed, download the installer or
   the portable archive from https://github.com/timoncool/YuE2-Studio/releases/latest (it needs an
   NVIDIA card; the first start offers to download the models).
2. Start it. The MCP server is up as soon as its window is: `http://127.0.0.1:8791/mcp`.
   Nothing else to install - no npx, no bridge.
3. Connect (below), then call `studio_status`. If the models are missing, `models_catalog`
   and `models_download` fetch them.

## Connect

```bash
claude mcp add --transport http yue2-studio http://127.0.0.1:8791/mcp
```

Other clients: `{ "mcpServers": { "yue2-studio": { "type": "streamable-http", "url": "http://127.0.0.1:8791/mcp" } } }`.

The server also serves this skill (resource `studio://skill`, prompt `studio`) and the
writing guides (resources `studio://guide/<topic>`). It speaks MCP `2026-07-28` (stateless:
every request carries its version in `_meta`, `server/discover` describes the server) and
the handshake revisions `2025-11-25`, `2025-06-18` and `2025-03-26` through `initialize`.
Only this computer's agents and the studio's own window may connect.

The user sees it in the studio too: Settings, **Agent (MCP)** shows whether an agent is
connected and the address to paste.

## Ground rules

- **Start with `studio_status`.** It tells what runs now and whether the window is open.
- **Long work is a job**: songs, scores, stems, MIDI, karaoke, preparation, training. Start it,
  then `studio_wait` (a `job_id`, or `until: stems | midi | processing | covers_and_karaoke |
  song_jobs | preparation | training | idle`) instead of
  polling. It returns within a minute (30 s by default, 55 at most) with how far the work got; call
  it again.
- **One heavy job holds the graphics card at a time.** While a LoRA trains no song is
  made; start training last.
- **Answers are short by default**: a song job is its status and the songs it made, a
  library song leaves out its audio codes, `lora_list` gives one line per LoRA. Pass
  `response_format: detailed` when you need every field.
- **Covers are drawn only with an image model set up** (`settings_get`, covers; an
  OpenRouter key). Without one a `cover_prompt` is kept but no cover appears;
  `cover_set_from_file` still works.
- **Look ids up, never guess them**: `library_songs_list`, `training_status`,
  `dataset_get`, `lora_list`, `models_status`.
- **What you change is written down where the user reads it.** Every change you make - a
  generation started, a song edited or deleted, a playlist, stems, karaoke, a dataset and its
  preparation, training, a LoRA, a model downloaded or removed, a setting - goes into the
  studio's Activity journal as what was done, to what and its name; a job is written as
  started. Reads, the window's own controls (`ui_*`, the player, the create form, the video
  editor, the visualizer) and answers that store nothing leave no line. A tool that writes
  over what was stored (a song or playlist edit, a dataset prepared again, stems made again,
  karaoke, MIDI, a cover) is marked destructive, so your client asks the user first.
- **A like lives with the song**: `library_liked` (the latest like first) and
  `library_song_like` work without the window open.
- **A stem belongs to its song**: the window shows it under the song it was separated
  from, `stems_split` on a song that has stems replaces the ones it makes again, and a stem
  itself is not split.
- **Files on this computer are passed by path**: `dataset_add_folder`,
  `library_import_audio`, `score_transcribe`, `cover_set_from_file`, `video_set`.
  `library_song_files` and `dataset_song_files` give the paths of the studio's own files.
- **You write, not the studio's assistant.** The studio has a small local model (Gemma)
  for users without an agent. You write better: read `writing_guide` and
  `writing_examples` first and write the style, lyrics and scores yourself. Use
  `assistant_write` only when the user asks for the studio's assistant.
  `assistant_sections` tags lyrics the user wrote with their sections without changing a
  word - the tag button of the create form.

## What YuE2 reads - read `writing_guide` for the full rules

- **style**: one English sentence in this order - language, genre with its era, vocal
  (register, gender, delivery), instruments named concretely, mood in two to four words,
  production, and last `N BPM`. No artist, song title, key or time signature. An
  instrumental says `instrumental` where the language goes.
- **lyrics**: sections tagged `[Intro]`, `[Verse 1]`, `[Pre-Chorus]`, `[Chorus]`,
  `[Bridge]`, `[Outro]`, one tag per line, a blank line between sections, about 2-3 sung
  words per second. Russian `ё` stays `ё`; a combining acute (U+0301) right after a vowel
  puts the sung stress on it where the model would stress the word otherwise.
- **abc**: the score the model sings from, in YuE2's dialect (`writing_guide` topic
  `score`). `score_compose` writes one to start from.
- **harmony** (song_create, score_compose): when the model writes the score and loops one
  progression, `strength` about 6-10 breaks the loop, `follow_lyrics: true` holds the score to
  the lyrics' sections in order, `section_strength` gives each section its own opening and
  `outside_bonus` brings in chords outside the key. Ignored with a given `abc`.
- `writing_examples` returns the official M-A-P requests closest to your idea - match
  their shape and density.

## Recipes

**A song from an idea**

1. `writing_guide` topic `song`, `writing_examples` with the genre and mood.
2. Write the style and lyrics yourself.
3. `song_create` (with `title` and `cover_prompt`), then `studio_wait` with its `job_id`.
4. `player_play` with the new song's id to let the user hear it; `ui_screenshot` shows it.

**A cover of a recording**

1. `score_transcribe` with `song_id` or `path`; `studio_wait` with its `job_id`.
2. `score_match_sections` with that `abc`, the original lyrics and the same `song_id` or `path`:
   it retags the lyric blocks with the sections they are sung in. Check the blocks it reports
   as `unsure`, then use its `lyrics`.
3. `song_create` with the new style, those lyrics and that score as `abc`.

**A LoRA from a folder of songs, written by you**

1. `training_status`. If the trainer or the listening pack is missing:
   `training_pack_install`, `training_listen_pack_install`.
2. `dataset_create` with the artist's name (the trigger word is made from it), then
   `dataset_add_folder` with the folder.
3. `dataset_prepare` with `lyrics: missing, style: missing, writer: agent`, then
   `studio_wait until: preparation`. The studio finds the lyrics in LRCLIB, QQ Music and
   Kugou, recognises only what they miss with Whisper, and listens to every song with
   MOSS-Music, measuring the tempo. It leaves the writing to you.
4. `dataset_get`. For each song:
   - `lyrics_state: found` - the words are there (from a database when `lyrics_source`
     names one: keep every word; `recognised`: fix the recogniser's mishearings). Lay them
     out in sections (`writing_guide` topics `sections` or `transcript`).
   - `style_state: heard` - `heard` holds what MOSS heard (genre, caption, bpm). Write the
     style sentence from it (`writing_guide` topic `style`), ending with that BPM.
   - Save with `dataset_song_update`; what you write is final and marks the song done.
   - `lyrics_state: wanted` after preparation: nothing found it. `lyrics_find` with other
     spellings, or ask the user, or write it instrumental.
5. `training_start` with `recipe_defaults` from `training_status`: preset `balanced`
   (100 steps of 4 songs; `fast` and `thorough` are the smaller and larger sizes), or
   `tuned`, the previous recipe with stop `kl` at 1.4 or stop `epochs`.
   `studio_wait until: training`; `training_status` shows step and loss (KL under `tuned`).
6. `training_checkpoint_install` for the chosen step, then `song_create` with that LoRA
   in `adapters` and its trigger word in the style.
7. Not there yet after the run? `training_continue` with `steps` above the run's
   `resume_step` from `training_status`: it goes on from the latest checkpoint with the
   same recipe and songs, and stops at that step (`resume_refused` says why a run cannot).

**The create page, where the user can see it**

`song_create` makes a song directly. When the user wants to watch and adjust it first:
`ui_navigate` create, `create_form_set` with the fields (the user sees them fill in),
`create_form_get` to check, and `create_form_submit` to press Create.

**When you are the studio's writing assistant**

The user can pick **Agent (MCP)** as the assistant engine. Then the studio's write buttons,
and the lyric layout and the styles of a dataset preparation, ask you instead of its local model:
`assistant_requests_wait` returns each request with the instructions and the answer schema
the local model would get; write the answer by them and send it with
`assistant_request_answer`. Keep calling `assistant_requests_wait` while the user works -
`studio_status` shows `assistant_requests_waiting`. A request waits 15 minutes.

**Talking to the user**

`ui_notify` shows the user a short message in the window. `ui_console` shows the errors the
window logged, when a button did nothing.

**A video clip**

The editor works in the studio's window, which must be visible while you edit and render:
a minimised window or a hidden tab holds the preview and the render.

1. `video_open` with a song id, `video_get` to see the presets and settings.
2. `video_set`: preset, aspect ratio, colours, effects, text layers, karaoke lyrics,
   a background picture or video from a path. `video_seek` and `ui_screenshot` to look.
3. `video_render`, then `video_get` until `export.saved` names the MP4 (or `export.error` says why not).

**Listening: equalizer, visualiser, Winamp**

- The day's best: `library_songs_list` with `since: "today"`, `library_liked` (the user's
  thumbs-up), then `player_play` with `song_ids` plays them as the queue.

- Equalizer: `equalizer_get` names every preset; `equalizer_set` with `preset` ("Rock",
  "Vocal Booster"...), or `bands` / `preamp_db`, `balance`, `mono`, `panel_open`,
  `save_preset`. `.EQF` files: `equalizer_import`, `equalizer_export`.
- Visualiser: `visualizer_set` with `place` (panel, window), `fullscreen`, `engine`
  (milkdrop, spectrum), `preset` (`visualizer_presets` searches them), `look`, `step`.
- Winamp mode, the whole window as a Winamp 2 player: `winamp_set` `on: true` (with a
  `skin` from `winamp_skins`), `on: false` to come back. While it is on, `player_*` drive it;
  `winamp_set` also opens its windows, shades them, sets its equalizer and MilkDrop,
  its `scale` (1.2 = 120 %) and `skip_menu`.
  New skins: `winamp_museum` opens the museum, `winamp_skin_add` takes a downloaded `.wsz`.

**Anything the tools do not cover**

`ui_read_page` lists every control of the window with a ref; `ui_click`, `ui_type`,
`ui_select`, `ui_press_key` work it like the user; `ui_navigate` and `ui_open_settings`
move around. Check the result with `ui_screenshot`.

## Tools by area

- **studio**: status, wait, system, capabilities, open data folder; **settings** get/set.
- **models**: status, catalog, download, adopt (files already on disk), select, cancel,
  remove; **engine**: options,
  restart, logs.
- **song**: create (playlist_id puts the made songs into a playlist), defaults (what a field left out becomes), job get/list/cancel, replay; **score**: compose, transcribe, job
  get/cancel.
- **writing**: guide, examples; **assistant**: write, status, set, runtime, models;
  requests wait and answer (when you are the assistant).
- **library**: songs list (since/until), liked, song like, song get/update/delete/files,
  import audio, versions, describe style (by ear, for a cover of a recording without one);
  **playlist**: list/create/update/delete.
- **cover**: draw, set from file, templates, prompt render; **karaoke**: make, delete,
  settings; **recogniser**: install/remove; **stems**: split, get; **separator**: status,
  install, settings; **midi**: status, transcribe, get, delete, install, remove, cancel; **processing**: start, get, keep, discard, reference; **vst**.
- **lora**: list, install from the catalogue or Hugging Face, import files, update,
  delete, export for ComfyUI (a trained LoRA as one file for ComfyUI's native YuE2).
- **dataset**: create, add folder or library songs, import, get, update, delete, song
  update/delete/files, prepare (+ cancel, train after), take as is (no assistant: found lyrics and heard styles kept as they are), reveal; **lyrics**: find;
  **training**: status, start, cancel, checkpoint install, run delete, packs.
- **ui**: screenshot, read page, click, type, select, press key, scroll, navigate, open
  settings, notify, console; **create_form**: get, set, submit; **player**: state, play, pause, seek, next, previous, set (repeat none, all, one, or stop after the track); **equalizer**: get, set,
  import, export; **visualizer**: get, set, presets; **winamp**: get, set, skins, skin add,
  museum; **video**: open,
  get, set, render, play, pause, seek, close.
- **openrouter**: status, key, catalog, log, complete, cover, transcribe.

## Turn a track into MIDI

1. `midi_transcribe` with `song_id` - a song, a stem, a processed take - or `path` of any
   audio file; `size` small, medium (default) or large. The transcriber and the model are
   downloaded the first time (0.1 GB plus 0.4, 1.2 or 5.5 GB); `midi_install` fetches them
   ahead. It runs on the card: NVIDIA from GTX 16 and RTX 20 on, driver 580 or newer.
2. `studio_wait until: midi`.
3. `midi_get` names the .mid on this computer, its model and instruments (34 groups and
   drums); `response_format: detailed` gives every note. `library_song_files` lists it too.
   A file named by path is written to the studio's `midi` folder (`midi_status` run.file).
4. The weights are MuScriptor by Kyutai & Mirelo, CC BY-NC 4.0: say so when the user wants
   the MIDI for commercial work.
