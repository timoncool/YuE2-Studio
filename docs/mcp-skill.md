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
- **The studio speaks five languages** (English, Russian, Chinese, Japanese, Korean) and the
  window follows the user's - so anything a person reads comes from `app/i18n/yue2.ts`, all five
  of them, never a literal in the code: a session name you choose for them, a hint, a warning.
  A session the studio keeps for itself is not named at all (see below).
- **Every track is made inside a session, and you say which one.** The makers that make or hold
  a track - `song_create`, `song_replay`, `stems_split`, `processing_start`, `karaoke_make`,
  `midi_transcribe`, `library_import_audio` - take a `session` word: `each` (a session of its own
  for every track, named after the track), `current` (the session open now), or `new:<name>` (one
  new session for the whole pack). Say it whenever the person has told you where the work goes;
  leave it out and the window asks them once for the pack. `score_compose` and `score_transcribe`
  do not take it: a score is not a track yet.
  `workspace_list` shows the sessions and which one is open; `workspace_create` makes one
  (it is made closed and waits for the person - only the window opens a session, so their work
  never switches behind their back), `workspace_open` brings an old one back, `workspace_close`
  files one away. A maker refuses to run while nothing is open and nobody has chosen: ask the
  person to open or make one in the window, then work.
- **Check where a track really landed.** A track goes into the session you named - not into
  whatever the window happens to have open - but a person moves things: after the job,
  `workspace_get` says which session holds the track. A `completed` job is not proof that the
  track is where you meant it, so look before you tell anyone it is done.
- **One session is the studio's own, not the person's.** Tracks that belong to no session at all
  are gathered into it once, and `workspace_list` shows it as `"kind": "import"` with an empty
  name: the words a person reads for it are drawn by the window in their language, so never look
  it up, or call it, by a name - not "Import", not "Импорт". Leave that session alone: do not
  delete it, do not rename it, and treat its tracks as unclaimed work rather than as someone's
  project. Renaming it is not yours to do - a person naming it turns it into an ordinary session
  of theirs, and the mark goes with the name they typed.
- **Tidy up your own empty sessions.** `workspace_delete` removes a session for good - the tracks it
  held stay in the library, only the session and its mark on them go. Remove the sessions you made
  and left empty; a session the person made is theirs to remove, so ask instead of tidying it away.
  Deleting cannot be undone, so the leash puts the question to them first - and about a deletion
  the question is one plain thing: do it, or do not. Nothing about it is remembered for later.
- **Ask before what cannot be undone.** The user keeps the agent on a leash, set in
  Settings, Agent (MCP): `free` (no questions), `risky` (the default: a deletion, an
  overwrite, closing a session, splitting a track are put to the user first) or `all`
  (everything that changes anything). Answers are kept where they belong, never in your
  hands: "in this session" is kept with the session itself, "always" is the studio's one
  switch for tracks, and "never" joins a list of forbidden tools in the settings - after
  that the tool does not run at all, whatever you ask. `agent_settings` shows the leash,
  the switch, what is forbidden and which sessions let you work, and `agent_leash` says
  how close you are kept. Never widen the leash yourself - only the window answers, and
  only the person in front of it sets the rule. A refusal is a fact, not a hint to retry.
- **Look ids up, never guess them**: `library_songs_list`, `training_status`,
  `dataset_get`, `lora_list`, `models_status`, `workspace_list`.
- **A stem is not a song, and the difference lives in the data**: a track made by
  `stems_split` carries `metadata.derived` (`tool: "stems"`, `from: <the song it came from>`,
  `settings.stem: drums | bass | vocals | guitar | piano | other`); a full song has no `derived`.
  Tell them apart by that marker only - never by the title, the file name or the file type:
  a stem is a normal library row with its own audio, and its file may be `.wav` while a song's
  is `.mp3`, which says nothing about what it is.
  **A stem is never counted as a song either.** It is shown inside the song it came from, so a
  session holding 18 tracks honestly reads as "12 songs + 6 parts" (the window counts the roots -
  `splitByParent` in `app/components/songParts.tsx`). When you report how much work a session holds,
  count songs, and say the parts beside them rather than adding them in.
  **An earlier set is kept, not thrown away.** Splitting a song again puts its six current stems
  into the archive with the moment the set was made, and a new set is made at once - so a person
  never loses a separation they may still want. `stems_archive_list` names the sets that were put
  away, each keeping the song it was separated from (`song_id` for one song, left out for every
  song); `stems_get` speaks only about the current set, and an archived stem is history, not a
  library row.
- **A date is not an opinion.** Two different moments live on a song: `updated_at` moves when the
  song itself is edited (title, cover, words, karaoke, the version chosen), while `metadata.liked_at`
  moves only when the person gives a thumbs-up - `library_song_like` and nothing else. A session's
  `updated_at` moves with any change to what it holds: a track added, removed or edited inside it.
- **What you do is written down where the person can read it.** Every tool that changes the
  studio - a track made, edited, deleted, a session opened, closed or removed, a split, a
  thumbs-up, an import, a job started - leaves a line in the studio's own message log, the bell
  in the top panel. It is written quietly: no toast interrupts the person's work. Reads
  (`*_list`, `*_get`, `*_status`) and `ui_*` change nothing and are not written down. A line
  names what was done, the kind of thing it was done to and that thing's own name, and the window
  draws the words in the person's language - so a tool's own name never reaches the log, and you
  should not speak in those names either. A deletion reads in the log exactly as it is put to the
  person: the same name, out of the same data.
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
  words per second. Russian `ё` stays `ё`.
- **abc**: the score the model sings from, in YuE2's dialect (`writing_guide` topic
  `score`). `score_compose` writes one to start from.
- `writing_examples` returns the official M-A-P requests closest to your idea - match
  their shape and density.

## Recipes

**A song from an idea**

1. `writing_guide` topic `song`, `writing_examples` with the genre and mood.
2. Write the style and lyrics yourself.
3. `song_create` (with `title` and `cover_prompt`), then `studio_wait` with its `job_id`.
4. `player_play` with the new song's id to let the user hear it; `ui_screenshot` shows it.

**Working inside a session**

1. `workspace_list` - what sessions exist and which one is open (`workspace_get` for one, and
   for the tracks it holds).
2. Where do these tracks go? Three words, said on the call itself (`song_create` and the rest):
   - `session: "each"` - the person wants a session per song: every track gets its own, named
     after the track.
   - `session: "current"` - into the session open right now.
   - `session: "new:<name>"` - all of them into one new session with that name.
   Say nothing and the window asks the person once for the pack of work at hand; their answer
   stands while you keep working, and `agent_settings` says whether they settled it for good.
3. Nothing is open and the person did not say? `workspace_create` makes a session (closed -
   only the window opens one) and `workspace_open` brings an old one back: the window does it,
   or the person does.
4. Make songs as usual. After each job, `workspace_get` confirms which session holds the track -
   and if the person moved it, say so instead of assuming. `workspace_close` files a session
   away; do it only when the user is done with it.

**A cover of a recording**

1. `score_transcribe` with `song_id` or `path`; `studio_wait` with its `job_id`.
2. `song_create` with the new style, the original lyrics and that score as `abc`.

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
- **workspace** (sessions): list, get, create, open, close, delete - where every track is made; the
  makers take `session: each | current | new:<name>`. `kind: "import"` marks the studio's own
  session (the one that gathers unclaimed tracks): it has no name of its own.
- **agent**: settings (the leash, the one switch for making tracks, what is forbidden),
  leash (how close it is kept).
- **cover**: draw, set from file, templates, prompt render; **karaoke**: make, delete,
  settings; **recogniser**: install/remove; **stems**: split, get, archive list (the sets put
  away when a song is split again, each keeping the song it came from); **separator**: status,
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
