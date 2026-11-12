---
name: yue2-studio
description: Drive YuE2 Studio on this computer through its MCP server - write and make songs with YuE2 (style, lyrics, scores, covers of recordings), manage the library, draw covers, split stems, turn any track into MIDI, time karaoke, process audio, make video clips, play songs, install LoRA, and build a LoRA from a folder of songs end to end, writing the lyrics layout and styles yourself instead of the studio's small assistant. Sees and works the studio's window like a user. Use whenever the user asks for anything the studio does.
---

# YuE2 Studio through MCP

YuE2 Studio serves MCP at `http://127.0.0.1:8791/mcp` while it is open (Streamable HTTP,
stateless JSON-RPC). Every tool runs the same code as a button of the studio, and the user
sees what you do in the studio's window.

## If the studio is not running yet

1. If it is not installed, take the package for this computer from
   https://github.com/timoncool/YuE2-Studio/releases/latest: the installer or the portable
   archive for Windows, the .dmg for a Mac with Apple silicon, the .deb or the AppImage for Linux
   (experimental). An NVIDIA card is not required: without one the engine runs on the processor,
   many times slower, and AMD and Intel cards go through Vulkan - see the README. The
   first start offers to download the models.
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
  Its `engine` is what the running engine started with: `max_batch_for_card` when fewer tracks a
  request than Settings asks fit the card. A song job says `tracks_asked` when it made fewer tracks
  than asked for that reason, and `card_memory_short` when the card's memory ran out and the driver
  lent the engine the computer's memory (the song goes on, many times slower). A training run says
  `step_stage`: the part of the step the trainer works on.
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
- **abc_continue** (song_create; `abc` on score_compose): the abc is only an opening - a hum
  from score_transcribe, a melody from the MIDI editor - and the model writes the rest of the
  song on from it.
- **score_chord_bed**: a score at once in a tempo, key and meter - chords over rests, one
  section per lyrics tag - for a song that keeps that tempo, key and chords; pass its `abc` to
  song_create. With words to sing the model plans the melody over the bed's chords (the bed's
  header opens the plan, each section is held to its chords) and the song's score is that plan;
  without words the bed is played as it stands.
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
   Kugou, recognises only what they miss with CrisperWhisper, and listens to every song with
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

- **studio**: `studio_status`, `studio_wait`, `studio_system`, `studio_capabilities`, `studio_open_data_folder` (in Explorer, for the user), `runtime_status` (midi, training pack, training listen), `system_gpus` (the cards and the one in use).
- **settings**: `settings_get`, `settings_set`.
- **network**: `network_proxy_get`, `network_proxy_set` (no proxy, the system's or your own, and the HF-Mirror), `network_proxy_test`, `network_access_get`, `network_access_set` (opening the studio from other computers, a new key).
- **journal**: `journal_read` (the Activity page), `journal_clear`, `journal_remove`.
- **models**: `models_status`, `models_catalog`, `models_download`, `models_adopt` (files already on disk), `models_select`, `models_cancel_download`, `models_remove`, `models_update_plan` (what this release updates, adds and no longer needs, with the reasons: the update screen), `models_update_apply` (its button: downloads, then removes what former releases left), `models_update_later` (its «Later»).
- **engine**: `engine_options_get`, `engine_options_set`, `engine_restart`, `engine_logs`. After the card ran out of memory on a song the studio makes it once more with a smaller KV cache and keeps that step for the card (`in_effect.max_seq_reason` is `fitted`); `engine_fitted_cache_reset` goes back to the model's full context.
- **song**: `song_defaults` (what a field left out becomes), `song_create` (playlist_id puts the made songs into a playlist), `song_job_get`, `song_jobs_list`, `song_job_cancel`, `song_replay`, `song_tokenize`, `song_job_dismiss` (an ended job's card off the page), `song_job_score_again` (a song failed with `score_unsingable_strength` made again with its LoRA at 1.0 on Composition), `song_job_again` (a song failed with `not_audio` made again with the same request).
- **score**: `score_compose`, `score_chord_bed`, `score_job_get`, `score_job_cancel`, `score_transcribe`, `score_match_sections`, `score_to_midi` (base64 .mid), `score_instrumental` (the voice's melody to the instrument), `score_vocal_octave`, `score_from_midi` (a .mid path), `score_mark` (sections from style and lyrics).
- **writing**: `writing_guide`, `writing_examples`.
- **assistant**: `assistant_requests_wait`, `assistant_request_answer` (when you are the assistant), `assistant_sections`, `assistant_write`, `assistant_status`, `assistant_set`, `assistant_local_models`, `assistant_cli_agents` (Claude Code, Codex, opencode, Antigravity, Muse Code: installed or how to install), `assistant_runtime`, `assistant_model_install`, `assistant_model_remove`, `assistant_cancel_download`, `assistant_start`, `assistant_stop`.
- **library**: `library_songs_list` (since/until), `library_liked`, `library_song_like`, `library_song_get`, `library_song_update`, `library_song_delete` (to the bin for 30 days, with its stems), `library_trash_list`, `library_trash_restore`, `library_trash_delete` (for good), `library_trash_empty`, `library_import_audio`, `library_file_tags_get`, `library_file_tags_set`, `library_song_files`, `library_version_select`, `library_version_delete`, `library_song_describe_style` (by ear, for a cover of a recording without one), `recording_describe_style` (the same for an audio file on disk), `library_song_note_set` (the song's note).
- **playlist**: `playlist_list`, `playlist_create`, `playlist_update`, `playlist_delete`, `playlist_get`.
- **cover**: `cover_draw`, `cover_templates_get`, `cover_templates_set`, `cover_set_from_file`, `cover_prompt_render`, `cover_look_get`, `cover_look_set` (the photo or pattern of tracks without a cover, unique: each track's pattern drawn from its words and music style, kept in the file or not), `cover_scenes`, `cover_photos_search`, `video_backgrounds_search`, `song_cover_photo_set`, `song_cover_pattern_set`.
- **karaoke**: `karaoke_make`, `karaoke_settings_get`, `karaoke_settings_set`, `recogniser_install`, `recogniser_remove`, `recogniser_cancel_download`, `karaoke_delete`, `karaoke_set` (the person's own .srt, .ass or .lrc, or edited LRC lines), `lyrics_recognise` (the lyrics a song or an audio file sings, heard by the karaoke recogniser).
- **stems**: `stems_split`, `stems_get`.
- **separator**: `separator_status`, `separator_runtime`, `separator_runtime_install`, `separator_cancel_download`, `separator_install`, `separator_remove`, `separator_settings_get`, `separator_settings_set`.
- **midi**: `midi_status`, `midi_transcribe`, `midi_get`, `midi_delete`, `midi_install`, `midi_remove`, `midi_cancel`, `midi_set` (a .mid of your own).
- **processing**: `processing_start`, `processing_get`, `processing_keep`, `processing_discard`, `processing_set_reference`.
- **vst**: `vst_list`, `vst_scan`, `vst_open_editor`.
- **lora**: `lora_list`, `lora_install_catalog`, `lora_search_hf`, `lora_hf_files`, `lora_install_hf`, `lora_update`, `lora_delete`, `lora_import_folder` (every LoRA of a folder, linked without a copy), `lora_import_files`, `lora_cancel_download`, `lora_export_comfyui` (a trained LoRA as one file for ComfyUI's native YuE2).
- **dataset**: `dataset_import_folder`, `dataset_reveal`, `dataset_song_files`, `dataset_prepare_train_after`, `dataset_get`, `dataset_create`, `dataset_add_folder`, `dataset_add_library_songs`, `dataset_update`, `dataset_delete`, `dataset_song_update`, `dataset_song_delete`, `lyrics_syllables` (each sung line with its syllables, as the score counts them), `lyrics_find`, `dataset_prepare`, `dataset_take_as_is` (no assistant: found lyrics and heard styles kept as they are), `dataset_prepare_cancel`.
- **training**: `training_pack_cancel`, `training_status`, `training_pack_install`, `training_listen_pack_install`, `training_start`, `training_continue`, `training_cancel`, `training_checkpoint_install`, `training_run_delete`.
- **ui**: `ui_screenshot`, `ui_read_page`, `ui_click`, `ui_type`, `ui_select`, `ui_press_key`, `ui_scroll`, `ui_navigate`, `ui_open_settings`, `ui_notify`, `ui_console`.
- **create form**: `create_form_get`, `create_form_set`, `create_form_submit`.
- **player**: `player_state`, `player_play`, `player_pause`, `player_seek`, `player_next`, `player_previous`, `player_queue_add`, `player_set` (repeat none, all, one, or stop after the track).
- **equalizer**: `equalizer_get`, `equalizer_set`, `equalizer_import`, `equalizer_export`.
- **visualizer**: `visualizer_get`, `visualizer_set`, `visualizer_presets`.
- **winamp**: `winamp_get`, `winamp_set`, `winamp_skins`, `winamp_skin_add`, `winamp_museum`.
- **video**: `video_open`, `video_get`, `video_set`, `video_render`, `video_play`, `video_pause`, `video_seek`, `video_close`.
- **openrouter**: `openrouter_status`, `openrouter_set_key`, `openrouter_catalog`, `openrouter_catalog_refresh`, `openrouter_log`, `openrouter_complete`, `openrouter_cover`, `openrouter_transcribe`.

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
