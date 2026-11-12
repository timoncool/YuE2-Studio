# Changelog

What changed, newest first. Dates are release dates; the studio is versioned by its
Windows build.

## Unreleased — 4.0.0

### Added

- **The models stay loaded between songs where the card has room**: Settings - Local engine has Keep models loaded - Auto, Yes or No - in place of the switch. Auto keeps both halves of the backbone and the VAE on the card when the set kept whole, a full-context cache for each track and a reserve fit the card's free memory at the engine's start, so the next song does not load them again; kept models are unloaded before stems, Audio to MIDI, karaoke, recognised lyrics or the writing assistant on the card, and after 10 minutes without a song. A switch saved on reads as Yes, off as Auto. Under the choice "Now: ..." says whether the running engine keeps them and why, with the GB it weighed.
- **The KV cache goes to Q8 by itself when F16 does not fit the card**: Settings - Local engine has KV cache - Auto, F16 or Q8 - with Auto by default. Auto keeps the cache in F16 while the model set and a full-context cache for each track of a request fit the card's free memory at the engine's start, and takes Q8 when they do not, before any track is given up. Q8 takes half the memory of the cache: in the studio's runs a 600-second song in BF16 peaked 1.3 GB lower and took 177 seconds instead of 188, a 120-second one 0.5 to 0.6 GB lower in about the same time; it works only with flash attention on an NVIDIA card (CUDA), and on the processor, Vulkan or with flash attention off the cache stays F16 with that reason. The same seed on another type of cache gives another song, so the song's parameters name its cache. The engine log says which cache a start took and why, in gigabytes, Settings shows it under the choice, and the window says once in a run of the studio when Auto has switched to Q8. Agents set it with `engine_options_set` and read it in `engine_options_get`.
- **Four more LoRA in the catalog**: 1980s Italo Disco (trigger `italodisco`) and 1980s Two-Tone Ska (trigger `twotone`) by James E. Petts, both CC0 sound adapters; Keygen Music by monsterovich, a composition and sound pair at AR 0.3 and NAR 0.5 trained on instrumentals; and Instrumental, no voice by stevensu (CC BY-NC), a composition adapter that keeps songs without lyrics from singing, humming or chanting. A LoRA whose strength lies in its file's metadata is now read at that strength.
- **Your own YuE2 model sings in place of the set's** ([#208](https://github.com/timoncool/YuE2-Studio/issues/208)): a YuE2-3B fine-tune in GGUF for yue2.cpp, such as goldhub/YuE2-RS-Finetunes, put in `models/yue2-cpp/own` is chosen under Settings - Models - Your own YuE2 model, and the engine runs it in place of the set's model with the set's VAE; Do not use goes back to the set's. The same list has Community models - the nine merges of Ruach Studio's YuE2-RS-Finetunes in Q8_0 (3.8 GB) and Q6_K (2.9 GB), the authors' choice first - which the studio downloads into that folder when one is chosen, with the percent, Cancel and the Hugging Face mirror, checked against the SHA-256 of a pinned revision; a downloaded one is removed to the recycle bin from the same panel. A file of your own the studio neither downloads nor checks, a chosen file that is gone stops the engine's start by its name, the decoder companion stays off in Auto with it, and LoRA training and dataset preparation still run on the set's model. The song's parameters name the file. Agents have `models_own_list`, `models_own_select`, `models_own_cancel`, `models_own_remove` and `models_own_reveal`.
- **On Linux songs are made on an NVIDIA card through CUDA**, as on Windows: the Linux engine had only Vulkan for every card. It now carries the CUDA 13 build (Turing and newer, driver 580 or newer) and the CUDA 12 build (Maxwell, Pascal, Volta and GTX 16, driver 525 or newer), chosen by the same table as on Windows, so Flash Attention and the KV cache in Q8 work there too; other cards stay on Vulkan. The CUDA runtime and cuBLAS of the build are downloaded from NVIDIA's packages the first time the engine starts on it, with the percent on the start screen, into one folder the audio.cpp server of stems, karaoke and audio to MIDI shares (CUDA 13: about 440 MB, CUDA 12: about 585 MB).
- **A computer without a graphics card to compute on is told what that means**, as in MiniMax Music3 Studio ([#224](https://github.com/timoncool/YuE2-Studio/issues/224)): where songs would be made on the processor - no card, or built-in graphics below the smallest set - the first screen's recommendation and Settings - Local engine say that this is many times slower, tens of minutes and hours, and that a discrete card is faster.
- **A crash of the studio leaves a trace on disk**, as in ACE-Step Studio and MiniMax Music3 Studio: `logs/yue2-studio.log` in the data folder records each start, a panic of any thread and, on Windows, an exception nobody handles (an engine library running an instruction this processor lacks, a bad access) with its module and offset, kept to 8 MB with the previous start's beside it; keys are masked.
- **The style's parts explain their fields** ([#211](https://github.com/timoncool/YuE2-Studio/issues/211)): under the singing language, the tempo, the voice and the sound of Edit style parts a short line in the window's language says what the field sets, what Automatic leaves to the model and what words to type there.
- **Training shows the part of the step it is working on** ([#166](https://github.com/timoncool/YuE2-Studio/issues/166)): under the bar of a run, "Step N of M · part k of K" names what the trainer does now, from the composition's passes to updating the adapter, and the bar moves between whole steps as the parts go by; a slow step no longer looks stuck. Agents read the same in the training status as `step_stage`.
- **A song that ran out of video memory says so**: on Windows with an NVIDIA card a song whose card memory is full does not fail - the driver lends it the computer's memory and it goes on tens of times slower, which looked like a song hanging for hours. The song's card now says "Out of video memory" with what to lower, and the engine log notes it once, while the song goes on. Before that, a request of several tracks makes only as many as the card's free memory holds for its length - a minute's song takes far less than a ten-minute one - and its card says "Fewer tracks" with how many were asked.
- **The MIDI editor splits and joins notes** ([#146](https://github.com/timoncool/YuE2-Studio/issues/146)): Split Notes (Ctrl+E) cuts the chosen notes at the playhead, or where the right-click menu was opened; Join Notes (Ctrl+J) makes one note of the chosen notes of each pitch, from the start of the first to the end of the last, and leaves a pitch alone when a note of it outside the choice lies between. Both are in the right-click menu and in Edit, and each undoes in one step.
- **The writing assistant follows your own rules**, as in ACE-Step Studio ([#209](https://github.com/timoncool/YuE2-Studio/issues/209)): Settings - Models - Writing assistant has Your own rules for the whole song, the style and the lyrics, added after the studio's guide and followed where the two differ, and Words to avoid, typed or loaded from a .txt file; editing a score and laying out recognised or published lyrics keep their words. Agents set them with `assistant_set`.
- **A long song keeps its level after the seam of its sound** ([#183](https://github.com/timoncool/YuE2-Studio/issues/183)): a song longer than the KV cache holds gets its sound in chunks, and the next chunk used to come out 1 to 3 dB quieter for some 20 seconds after the seam. Each next chunk now hears 30 seconds of the song again before it is faded in, which keeps the level after the seam within half a decibel of a song made in one piece, for about 5 seconds more on a 4-minute song. Settings - Local engine has the overlap beside the KV cache, from 0 to 45 seconds.
- **Your own subtitles become a song's karaoke, and its lines can be edited** ([#218](https://github.com/timoncool/YuE2-Studio/issues/218)): on the song panel, Load .srt / .lrc takes a .srt, .ass, .ssa or .lrc file as the song's karaoke in place of the recognised timings, asking first when the song has karaoke already; Edit lists the karaoke's lines with their starts, and a line changed there is saved as written while the lines left alone keep the times of their words. A file reads in the encoding it was saved in: UTF-16 or UTF-8 with its mark, plain UTF-8, or the system's code page such as Windows-1251, GBK or Shift-JIS. Agents have `karaoke_set`.
- **A deleted song goes to a bin and comes back from there**, as in ACE-Step Studio ([#175](https://github.com/timoncool/YuE2-Studio/issues/175)): Delete puts a song in the Bin tab of the library with its stems, its karaoke and its versions, the message after it offers Undo, and Restore on the tab brings it all back for 30 days; after that, or with Delete for good and Empty the bin, the song leaves the library and its files go to the system recycle bin. A cover, a re-render, stems, MIDI, karaoke, a cover image or a video of a song in the bin is refused by its name until it is restored, in the window's language. Agents have `library_trash_list`, `library_trash_restore`, `library_trash_delete` and `library_trash_empty`.
- **Lyrics are heard from a recording on this computer**, as in ACE-Step Studio ([#222](https://github.com/timoncool/YuE2-Studio/issues/222), [#207](https://github.com/timoncool/YuE2-Studio/issues/207)): Tools - Recognise speech can use the recogniser chosen for karaoke (Parakeet or Whisper) as well as OpenRouter, the karaoke recogniser first when one is chosen; and in Cover a button above the lyrics hears what the source sings and puts it in the lyrics to edit, asking first when the lyrics are not empty. The words are laid out in lines: a sentence's end closes a line, a pause opens one, a long line is cut after its last comma. Agents have `lyrics_recognise`.
- **Flash attention can be switched on by hand** ([#228](https://github.com/timoncool/YuE2-Studio/issues/228)): Settings - Local engine has Auto, On and Off in place of the switch that could only turn it off. Under Auto flash attention is on again on RTX 50 cards, as in 3.8.3, where turning it off took twice the time and memory; if in the second half of a song the words turn into meaningless syllables, choose Off.
- **Settings - Local engine says which KV cache the engine runs with and why** ([#203](https://github.com/timoncool/YuE2-Studio/issues/203), [#228](https://github.com/timoncool/YuE2-Studio/issues/228)): an empty field read 24576 while a smaller cache was in use. The field now reads Auto, a line under it names the cache of the running engine (the model's full context, or set by you), and a saved value that has not reached the engine yet says it applies at the next start.
- **The minimum of the sound stage explains itself and warns before it cuts a song off** ([#225](https://github.com/timoncool/YuE2-Studio/issues/225)): a Min tokens close to the length limit kept the song from ending - the model sang the lyrics, then filled the rest with near silence and was cut off at the limit. The field now says what the number means (25 frames are one second; empty lets the model end the song itself), and a minimum of 90 % of the length or more is flagged under it.

### Changed

- **Karaoke and the dataset lyrics hear with CrisperWhisper**, as in ACE-Step Studio and MiniMax Music3 Studio: the Whisper choice is now CrisperWhisper 2.0 turbo, a Whisper large-v3 turbo trained for the time of every word, run in the audio.cpp server the stems and Audio to MIDI already use - on the card's CUDA, Vulkan on another card, the processor or Metal - in place of Faster-Whisper and its CUDA 11 libraries. Segments the model fills a gap with are left out. The update screen downloads CrisperWhisper and then removes Faster-Whisper; a Whisper size left in the settings no longer names a model.
- **Audio to MIDI shows its notes as it hears them** ([#185](https://github.com/timoncool/YuE2-Studio/issues/185)): the player draws the notes while MuScriptor works and says which chunk it is on, instead of staying empty until the end; the finished MIDI is the same as before.
- **The where-it-runs table says what the studio picks on AMD and Intel cards**: HT-Demucs separates the stems through Vulkan there, Mel-Band RoFormer and MuScriptor run on the processor; the table under Settings - Models and the README said the stems were separated on the processor.
- **The README's tables of files to download by hand name what the studio downloads**: transcribe.cpp 0.2.1 archives, audio.cpp's server for HT-Demucs and MuScriptor, and HT-Demucs as audio.cpp's GGUF, where they still named the ONNX HT-Demucs and HOT-Step's MIDI transcriber.
- **The MIDI editor is updated to the latest signal**, as in ACE-Step Studio: rewind after recording, the direction of curves and lines, the shortest note length and the rest of upstream's work since our last update, with the studio's own mode, chord and section lanes kept.
- **The library search finds a song by any of its words**, as in ACE-Step Studio: the words of a search may come in any order, and it reads the lyrics, your note on the song and the settings it was made with - the seed, the model and the LoRA - besides the title, the style and the tags.
- **The form's and the song panel's blocks fold and stay folded**, as in ACE-Step Studio ([ACE-Step Studio #63](https://github.com/timoncool/ACE-Step-Studio/issues/63)): in the form the style, the lyrics, the score, LoRA and quality fold by their title, and in the song panel Style, Note, Parameters and Lyrics do too; the choice is kept across restarts, and Parameters opens folded until you open it.
- **Tools opens on the song that stems or MIDI are working on**, as in ACE-Step Studio: the progress is in view at once, and the chosen song always stays in the Tools list, first when it would be past the twelve songs the list shows.
- **The agent's guide names every platform the studio ships for**, as in ACE-Step Studio: the first step of the MCP skill said the studio is a Windows program that needs an NVIDIA card; it now points an agent to the Windows, macOS (Apple silicon) and Linux (experimental) packages and says the engine also runs on the processor and on AMD and Intel cards through Vulkan. The README adds the owners' reports of a Radeon RX 7900 XT under Windows and an RX 7900 XTX under Ubuntu 26 ([#196](https://github.com/timoncool/YuE2-Studio/issues/196)).
- **A download that a proxy does not let through names the proxy**, as in MiniMax Music3 Studio: when the studio goes out through the system's proxy or the one set in Settings - Providers and that proxy does not take the connection at all, the message names the proxy by its address (never its login) and says what to do - start the proxy program, or choose No proxy - instead of suggesting HF-Mirror, which sits behind the same proxy. A proxy that takes the connection but cannot reach the site is not blamed, and the mirror is still tried through it. A model download that failed now shows its reason in Settings - Models too, not only on the first start.
- **Engines and tools up to date**: the assistant on llama.cpp b12248 (b11900 before; CUDA 13.5 on Windows and Linux, the same Ubuntu 22.04 limit for its CUDA build), tempo, key and the listening pack on ONNX Runtime 1.31.1 (1.30.0 before) with its Rust binding ort 2.0.0-rc.15, Parakeet on transcribe.cpp engine-0.2.1 and stem separation, karaoke and audio-to-MIDI on the audio.cpp server server-31cb1a4a. The update screen offers them as a components update. On Windows the assistant's CUDA 13 build now comes with the CUDA 13.5 libraries of the same llama.cpp release, so a computer that runs the assistant on CUDA 13 downloads 426 MB of them again.
- **On the processor the sound of a song is computed about 1.2 times faster**: the engine now uses ggml's tinyBLAS matrix kernel on the processor for the Q8_0 and BF16 sets, as llama.cpp does; the same seed on the processor gives another song than in 3.8.5.
- **The tempo of a song is measured on the processor**: in 5 to 7 seconds a song, for Describe by ear and dataset preparation alike. ONNX Runtime no longer runs on the graphics card, and the update screen removes the CUDA, cuDNN and DirectML files of earlier versions: about 3.1 GB on Windows with an NVIDIA card, about 36 MB with DirectML.
- **The writing assistant lets llama.cpp place its layers**: the studio no longer asks for every layer on the graphics card, which on a card of 8 GB or less could stop Gemma from loading, as it did in Dub Studio.
- **Studio Hub notices follow what you do at once**: a notice closed or clicked, and a song that ends, bring the next notice without waiting for the minute's refresh; a link in a notice is counted by its address, and the hub's requests wait while a song is made.
- **A score lost to a strong LoRA says so and makes the song again in one click**: when the LoRA on Composition add up to more than 1.2 and the model could not write the score, the song's error names their strength and offers to make it again with Composition at 1.0 (two LoRA share it, 0.5 each; the sound keeps its strengths and the form keeps yours). The form warns under the LoRA when Composition is set that high. In the studio's own runs one LoRA at 1.2 wrote the score and at 1.5 did not, and two at 1.0 each lost the key every time.
- **A score that lost only its header is kept more often**: the engine also drops the stray text the model wrote in the header before filling in the meter, length and key, where such a score used to fail.

### Fixed

- **A screen reader can play a song from the library**, as in MiniMax Music3 Studio ([#171](https://github.com/timoncool/YuE2-Studio/issues/171)): the play control on a song's cover in the library, a playlist and Liked is a real button named by the song ("Play: <title>", "Pause: <title>" while it plays), reached with Tab and pressed with Enter or Space.
- **On Linux the audio.cpp server finds its CUDA libraries**, as in ACE-Step Studio and MiniMax Music3 Studio: the CUDA 13 and CUDA 12 servers behind stems, karaoke and audio to MIDI had no CUDA libraries beside them on Linux and could not start on an NVIDIA card; each now downloads its cuFFT from NVIDIA's packages into its folder and takes the CUDA runtime and cuBLAS from the engine's (see the CUDA engine on Linux above), and is started with both folders on its library path.
- **An error stays on screen until you close it**, as in ACE-Step Studio ([#230](https://github.com/timoncool/YuE2-Studio/issues/230)): an error message vanished after three seconds, often before it could be read; it now stays until closed, a long one scrolls and its text can be selected and copied. Ctrl with the wheel scales the interface over every panel, and an assistant that writes too slowly to finish in three minutes says so instead of reporting that it cannot be reached.
- **A song over a chord bed is sung to a melody** ([#223](https://github.com/timoncool/YuE2-Studio/issues/223)): the bed went to the model as the song's whole score, with the Vocal voice at rest in every bar, so the words were chanted on two or three notes and the song's score showed rests only. A bed under words to sing now opens the plan with its tempo, key and meter and holds each section to its chords, and the model writes the melody over them: the song keeps the score with that melody. An instrumental is played from the bed as before.
- **A cloud recogniser that returns no timings no longer makes a dataset song an instrumental**, as in MiniMax Music3 Studio: an OpenRouter answer without timed words is heard again with the vocal lifted out, as a local "no words" is, and then says to pick a model that returns timings instead of marking the song instrumental; while a LoRA trains, the vocal is lifted out on the processor, a separator that fails leaves no temporary file, and a second hearing that fails for another reason says so.
- **Tools says when the library does not answer**, as in MiniMax Music3 Studio: the song list shows a spinner until the library answers, and an error with Refresh instead of "No songs yet" when it does not.
- **The window keeps up with a slow service**, as in MiniMax Music3 Studio: the setup status, the engine start, the tools, LoRAs, training, the agent panel and the engine log were asked again on a timer whether or not the last answer had come; each is now asked again a pause after its answer, one at a time.
- **A song no longer comes out silent when the card computes something other than sound**: the engine computes such a step again up to three times, and a song still left with numbers that are not sound fails with that reason and a Make again button instead of being saved as silence. How often a step had to be computed again is noted in the studio's log and counted in the anonymous statistics. Agents have `song_job_again`.
- **A training run the studio closed on goes on to where it was meant to end** ([#221](https://github.com/timoncool/YuE2-Studio/issues/221)): Train further on such a run now offers the steps it was started for, not four checkpoints more, and says "The studio closed at step N. Training goes on from checkpoint M up to K."
- **Training on an RTX 20xx card says why it is slow** ([#199](https://github.com/timoncool/YuE2-Studio/issues/199)): when the trainer computes the layers that card has no BF16 for in F32, the run's card says training goes several times slower there and still finishes.
- **A scaled studio in a browser tab fits the window**, as in ACE-Step Studio ([#165](https://github.com/timoncool/YuE2-Studio/issues/165)): at an interface scale other than 100 % the studio and its dialogs fit the tab, with the player at the bottom and no scrolling of the page, and the "slide to turn on" knob and a click on the MIDI player's notes follow the pointer.
- **What the studio starts ends with it, and what it opens for you stays**, as in ACE-Step Studio: the engine, the assistant's server and the other helpers end when the studio closes, also on Linux and macOS and before the update installer runs, while a file manager the studio opened stays open after it closes.
- **Updating karaoke's engine while karaoke has run no longer breaks it**, as in MiniMax Music3 Studio: an install over the engine the studio has loaded stopped at its first file, having removed some of the earlier ones, and karaoke reported the model missing until the next start. Files a running studio holds are now moved aside and removed at the next start, the new engine lands whole, and the update screen says it is used from the next start of the studio. A reserved port range is also found on a Windows in another language.
- **The karaoke of a video moves up to 30 seconds either way, in hundredths** ([#231](https://github.com/timoncool/YuE2-Studio/issues/231)): the lyrics offset in the video window reached only 3 seconds in tenths; it now takes from -30.00 to +30.00 seconds, on the slider and typed in its field.
- **Karaoke on lyrics longer than the song times only what is sung**: when the text repeats verses the song never reaches, every lyric line was put on one moment of the song together with the sung ones. The sung lines now get their own times from the first copies of the text, and lines the song does not reach get none; a first line sung unclearly keeps its place from the first word heard.
- **An instrumental writes a new score each time and keeps No score** ([#217](https://github.com/timoncool/YuE2-Studio/issues/217)): the score written for an instrumental stayed in the form, so the next instrumental was made from the same score, and No score was switched back to Melody + chords. The score written for an instrumental now belongs to that song only, a score you put in the form stays as you wrote it, and No score makes the instrumental without one.
- **Lyrics edited after Reuse prompt stay when you come back to Create** ([#216](https://github.com/timoncool/YuE2-Studio/issues/216)): leaving Create and coming back loaded the reused song's lyrics over your edits; a Reuse now loads the song once.
- **Cloud requests ride out a busy moment**, as in MiniMax Music3 Studio: an OpenRouter request (the assistant, descriptions, covers) that the service answers with 408, 409 or 429 is sent again after the wait it asks for, at most a minute, up to four times; a server that cannot be reached is named by its host.
- **A graphics card chosen in Settings that is no longer in the computer is said so**, as in MiniMax Music3 Studio: the studio no longer points CUDA at a card number that is not there (which left every CUDA part of the studio without a card, and the engine went to the processor without a word); the engine start says the chosen card is not in this computer, Settings - Local engine says so too and asks to choose one, and the engine starts once one is chosen. When nvidia-smi does not answer, the choice is left as it was.
- **The first start says when the engine computes on the card through Vulkan**, as MiniMax Music3 Studio does: next to the card the recommendation is sized for, an AMD or Intel card on Windows and every card on Linux are named as computing through Vulkan.
- **No Windows Terminal tab is left open beside the studio**: on Windows 11 24H2 and later the studio started from a shortcut gets no console of its own and makes one without a window, which the engine and the other programs it starts share, so neither a console window nor a Terminal tab appears; started from a terminal, it still writes its log there. On Windows 11 22H2 and 23H2 with Windows Terminal as the default terminal the tab still opens, as before: those versions have no way to start a console without one.
- **Stems and Audio to MIDI at the same time no longer break each other**: audio.cpp's server at work is never replaced under a run - a run takes it as it is when it carries its model, else waits for the other run to end - and stopping one panel's download of the server leaves the other's going; a run stopped while it waits ends at once.
- **A song is no longer refused because its style, lyrics and score leave too little room for the length asked** ([#203](https://github.com/timoncool/YuE2-Studio/issues/203), [#205](https://github.com/timoncool/YuE2-Studio/issues/205), [#206](https://github.com/timoncool/YuE2-Studio/issues/206), [#213](https://github.com/timoncool/YuE2-Studio/issues/213), [#182](https://github.com/timoncool/YuE2-Studio/issues/182)): 3.8.4 and 3.8.5 held cards of 8 GB or less, and RTX 50 cards of up to 16 GB, to a 9216-token context and stopped a song whose prompt and score left less room than its maximum duration. Every card starts on the model's full 24576-token context again, as in 3.8.3, and when a smaller context is set the engine sings the song as far as the room goes instead of refusing it.
- **A song that stopped at its limit is marked, not mistaken for a finished one**: the library shows "Cut short" on it, with the reason - the length limit (with a click to make it longer) or the style, lyrics and score taking too much of the model's memory.
- **A long song without flash attention keeps its length without running out of memory** ([#201](https://github.com/timoncool/YuE2-Studio/issues/201), [#228](https://github.com/timoncool/YuE2-Studio/issues/228)): without flash attention the sound is now rendered in pieces sized to the card's memory (on a 16 GB card about three minutes each) instead of shrinking the whole context, so the song keeps its full length; a 240-second song rendered in 120-second pieces peaked at 8.2 GB of card memory instead of 14.3 GB in one piece.
- **A song the graphics card runs out of memory on is made once more instead of lost**: the studio starts the engine again with the next smaller KV cache (16384, then 12288, then 8192), makes the song again and keeps that size for this card and model set. The journal says so, Settings - Local engine shows the size as chosen after the card ran out of memory with a button to go back to the full context, and a cache set by hand is never changed. A song stopped by you, or lost while training or stem separation shared the card, is not made again.
- **Describe by ear keeps to the processor when you chose it** ([#204](https://github.com/timoncool/YuE2-Studio/issues/204)): with the processor chosen in Settings - Engine the captioner still ran on the graphics card with its CUDA libraries; it now runs on the processor then, without them.
- **A program of the studio that crashes without a word says where it crashed** ([#204](https://github.com/timoncool/YuE2-Studio/issues/204), [#215](https://github.com/timoncool/YuE2-Studio/issues/215), [#181](https://github.com/timoncool/YuE2-Studio/issues/181)): when the engine fails to start, or the captioner or the trainer stops on an exit code such as 0xc0000005 or 0xc000001d before writing anything, the studio reads Windows' record of the crash and adds the module it happened in, the exception and the offset to the error and the engine log - the file name only, never a path.

## 2026-11-06 — 3.8.5

### Fixed

- **A song no longer runs out of memory on a 12 GB Blackwell card** ([#201](https://github.com/timoncool/YuE2-Studio/issues/201)): 3.8.4 turned flash attention off automatically on Blackwell cards (#194), which raised memory use enough to crash a 12 GB RTX 5070 that had worked in 3.8.3. The key-value-cache cap that already protected 8 GB cards now also applies whenever flash attention is forced off, on any card up to about 16 GB, so the song fits. A cache size set in Settings - Engine still wins.
- **LoRA training runs on Turing cards again** ([#199](https://github.com/timoncool/YuE2-Studio/issues/199)): on RTX 20-series cards (compute 7.5) the trainer stopped with "backend does not support" when a ConvRot8 step needed an F32 result; it now falls back to the F32 path on those cards instead of failing.
- **A long song no longer has an audible seam where its chunks meet** ([#183](https://github.com/timoncool/YuE2-Studio/issues/183)): the acoustic chunks now overlap and crossfade, so two chunks join without a click or a jump; this also smooths the problem behind [#145](https://github.com/timoncool/YuE2-Studio/issues/145).
- **The Hugging Face mirror is used automatically when Hugging Face cannot be reached**: on a network that cannot open huggingface.co — the most common setup failure in the telemetry, networks behind the Great Firewall among them — the studio now falls back to hf-mirror.com on its own, so the models and tools download instead of the setup stalling.

## 2026-11-05 — 3.8.4

### Fixed

- **The second section and the chorus no longer come out as sound-alike nonsense on newer NVIDIA cards** ([#194](https://github.com/timoncool/YuE2-Studio/issues/194)): the 3.8.0 move to a newer ggml brought a CUDA flash-attention kernel that is numerically wrong on Blackwell-generation cards (compute capability 10.0 and up, such as the RTX 5060 Ti), so a song sang the words right at the start and drifted into "sounds like singing but means nothing" later. The studio now turns flash attention off automatically on these cards; other cards are untouched and their speed does not change.
- **Songs are no longer lost to an out-of-memory crash on cards of 8 GB or less**: the smaller key-value cache that kept AMD cards of 8 GB or less stable was never applied to small NVIDIA cards, which crashed with an illegal-memory error mid-song (seen across many crash reports). The cap (9216) now covers any card of 8 GB or less; a cache size set in Settings - Engine still wins.
- **A finished song is no longer lost when the system temp folder cannot be written**: the FLAC was written through a temporary file, and a temp folder gutted or blocked by Windows lost the song at the last step. It is now encoded in memory.
- **Karaoke times original songs whose voice is buried under the mix** ([#192](https://github.com/timoncool/YuE2-Studio/issues/192)): when nothing is heard in the mix and the vocal separator is installed, the voice is lifted out and timed on its own, so a freely generated original is timed where the mix alone reported nothing - now with cloud recognition too.
- **Exported karaoke subtitles end when the line is sung, not at the next line** ([#189](https://github.com/timoncool/YuE2-Studio/issues/189)): SRT and LRC leave instrumental stretches empty instead of holding the last line, and the timings line up with the song.
- **Your chosen maximum duration is kept when a new Create loads a request** ([#184](https://github.com/timoncool/YuE2-Studio/issues/184)): reuse, an example or the writing assistant no longer resets it to the default.
- **The autogenerated lyrics fall back to your language instead of Russian** ([#198](https://github.com/timoncool/YuE2-Studio/issues/198)): a text-language selector was added, and under "Automatic" the lyrics follow the studio's own language when the request does not settle one.
- **Trained LoRA checkpoints show up again** ([#177](https://github.com/timoncool/YuE2-Studio/issues/177)): the studio now also finds checkpoints saved under the trainer's newer names (`*_joint_{ar,nar}_step<N>`), not only the older `native-{ar,nar}` ones.
- **Each changed component on the update screen has its own line** ([#180](https://github.com/timoncool/YuE2-Studio/issues/180)) instead of repeating the HT-Demucs description.
- **A score whose M:/L:/K: header is dropped is sung instead of thrown away**: the engine fills a default time, note length and key as a last resort.
- **The Russian typo in the score-edit assistant's placeholder is fixed** ([#195](https://github.com/timoncool/YuE2-Studio/issues/195)).

### Added

- **A 1980s Eurodisco LoRA is in the adapter catalogue.**

## 2026-10-28 — 3.8.3

### Fixed

- **Every engine runs on any x86-64 processor, with or without AVX2** ([#181](https://github.com/timoncool/YuE2-Studio/issues/181)): the engines are built as all-variants packages with no processor floor, so the 0xc0000005 crash on CPUs without AVX2 (such as older Intel i3) is gone.

## 2026-10-27 — 3.8.2

YuE2 Studio, ACE-Step Studio and MiniMax Music3 Studio are on one version again: 3.8.2 follows 3.8.0 here.

### Fixed

- **The engine starts when its port is taken or reserved**: the engine listens on port 18087, and when another program held it - or Windows had reserved it for Hyper-V, WSL or Docker - every song was lost with "cannot bind 127.0.0.1:18087". The studio now checks the port before the engine starts and, when it cannot have it, gives the engine a free one; the engine log says which program held 18087 (its name and PID) or which reserved range it lies in, and a song lost to a port taken later says so in the window's language.
- **A hub popup stays on screen once shown**, until it is closed, rather than going away when a song starts.
- **An M4A of Apple Lossless is kept as FLAC**: only an M4A or MP4 whose audio is AAC plays in the window as it is.
- **Downloads check, unpack and record their files off the service's workers**, so a gigabytes-large check no longer holds up the window's other requests; the update screen's progress keeps polling while the window around it redraws.

## 2026-10-27 — 3.8.0

### Fixed

- **Stems and Audio to MIDI on a computer with the CUDA Toolkit**: audio.cpp's CUDA 13 server looked for cuBLAS 13 beside itself and beside the engine only, while the engine takes it from an installed CUDA Toolkit and downloads none; the server then counted as not installed and HT-Demucs refused to run. The libraries on PATH, which the server loads too, now count.
- **The assistant starts on Linux with a glibc older than 2.38**: the CUDA builds of llama.cpp need glibc 2.38, so on Ubuntu 22.04 and the like the assistant did not start; there it now runs the card through the Vulkan build, and its settings say why, as in Speech Studio.
- **Programs started from the Linux AppImage no longer load its libraries**: the file manager, nvidia-smi, llama-server, Whisper, the trainer, the captioner and the command-line agents were started with the image's library path and could stop on a newer system («undefined symbol»); they start without it, as in Dub Studio.
- **A program downloaded without its execute bit runs on Linux and macOS**: an archive packed on Windows carries none - audio.cpp's Linux server among them - and the program answered «Permission denied» or was left out of the unpacking; the studio makes it executable.

- **A Radeon of 8 GB or less renders without flash attention and with a smaller cache**: on these cards Vulkan's flash attention made each step several times slower ([#86](https://github.com/timoncool/YuE2-Studio/issues/86)), and AMD cards of 8 GB or less were where songs were lost to "ggml_vulkan: device lost". The engine now starts there with flash attention off and a cache of 9216, the settings an RX 6650 XT user found fastest and stable; a cache size set in Settings - Engine still wins.
- **Style by ear fits the card it runs on** ([#149](https://github.com/timoncool/YuE2-Studio/issues/149), [#168](https://github.com/timoncool/YuE2-Studio/issues/168), [#169](https://github.com/timoncool/YuE2-Studio/issues/169)): MOSS-Music's memory grows with what it hears - from 11 GB for a minute to 18 GB for seven - and a song of five minutes ran a 16 GB card out of memory, while a card under 12 GB could not load it at all. The studio now gives it as much of the middle of a song as the card holds; a card under 12 GB hears with MOSS-Music's smaller Q4_K_M build (5.2 GB instead of 8.7, downloaded with the pack), and a card under 8 GB or one without CUDA 13 describes on the processor, two minutes of each song in a few minutes.
- **Closing the studio on a Mac stops the engine** ([#147](https://github.com/timoncool/YuE2-Studio/issues/147)): on macOS nothing ends a program with the one that started it, so the engine stayed after the studio closed, holding its memory, and the next start took it over with the models it had been started with - SheetSage2 downloaded since was not in it, and Cover said there was no transcriber. The studio now stops the engine as it closes, on every system.
- **Playback in the Linux AppImage run extracted** ([#157](https://github.com/timoncool/YuE2-Studio/issues/157)): the AppImage's GStreamer start-up script runs before the one that finds the AppImage's own folder, so in an extracted AppImage GStreamer looked for the computer's own plugins, loaded a second libsoup into WebKit's media process, and the window froze at the first press of play. The studio now points GStreamer back inside the AppImage before the window opens, and keeps its plugin registry apart from the computer's.
- **Instrumental only renders a score the model wrote without a voice** ([#148](https://github.com/timoncool/YuE2-Studio/issues/148)): with Instrumental only on and no score, Create first writes one, and from lyrics of section tags alone the model often writes no Vocal notes at all; moving the voice to the instrument then refused the score as "instrumental already", and Create stopped without a song. Such a score is now rendered as it is.
- **Every download is checked against the hash its publisher states**: a runtime, a model or a tool the studio fetches - from GitHub releases, Hugging Face, NVIDIA, PyPI - is compared with the SHA-256 its source publishes for the pinned version; a file that differs, cut off or altered on the way, is removed and the download says so with both hashes, instead of being installed. A file another program of timoncool's already has is taken only when it is the same bytes. cuBLAS 12.9 for karaoke and the stems' runtime now comes from NVIDIA's PyPI package, which has a published hash; the files are the same, and a computer that has them from 3.7.0 keeps them without a new download.
- **Karaoke and the stem separator load ONNX Runtime on a computer without the Visual C++ Redistributable**: ONNX Runtime imports msvcp140_1.dll, which the studio did not ship, so it loaded only where Windows already had it; the library now travels beside the engine with the rest of the Visual C++ runtime, and ONNX Runtime finds it there.
- **The style suggestions and the genres of Search speak the window's language** ([#152](https://github.com/timoncool/YuE2-Studio/issues/152)): the style phrases and genres stay in English, the language the model reads its style in, and a window in Russian, Chinese, Japanese or Korean shows each one's meaning beside it - in the suggestions of the style's voice and sounds and on the genre tags of the Search page (dubstep дабстеп, 回响贝斯, ダブステップ, 덥스텝).
- **Every window language is named in its own language**: Settings - Appearance - Language wrote Russian in the window's language («俄语» in a Chinese window, "Russian" in an English one) and, in English, Chinese, Japanese and Korean too, so a reader looking for their language did not always find it; the list now reads Русский, English, 简体中文, 日本語, 한국어 in every window.
- **A download from a server this network cannot reach fails in a minute and a half, not six, and names the server**: a connection that never opened - blocked, refused, timed out, no DNS answer - was tried eight times, 30 seconds each plus waits, before the setup said "could not start ... after 8 attempts"; it now gives up after three such attempts and says "could not connect to <host>", so it is clear whether Hugging Face, GitHub or NVIDIA is out of reach. For Hugging Face the message still points to the HF-Mirror in Settings - Network.
- **The device chosen for the engine is the one it computes on**: since the engine update it reads the device only from `--device`, and the studio still named it the old way, so CUDA, Vulkan or the processor chosen in Settings, Engine was ignored and the engine took its own pick. It is now passed as `--device`, and a device the engine does not have is named at the start with the ones it sees.
- **A failed song's reason is found in a long engine log**: the reason was looked for in the last 512 lines the window follows, and a song with LoRA writes more than that, so its start was gone and the song failed with no cause; the engine log file is read instead.
- **A CUDA failure names the card**: a song lost to a CUDA error says the card, its compute capability, the driver and the engine build it ran, which tells a card the build has no code for from a driver too old for it; a CUDA device the engine cannot open says what CUDA answered.
- **The engine opens files in folders named in any language**: on Windows its tools read paths through the system's ANSI code page, so a folder or file name with characters outside it (Chinese on a Russian or English Windows, Cyrillic on an English one) reached them as "?" and was not found; the engine now runs with the UTF-8 code page.
- **Every missing model file can be downloaded by hand**, as in MiniMax Music3 Studio: under the chosen set, on the first-run page and in Settings - Models, each file not on disk is listed with its exact name and size, its address at the pinned revision on Hugging Face (and on HF-Mirror when that is the source), a copy button and the folder it goes into. The writing assistant's Gemma models are pinned to a revision too, so a re-upload on their repositories cannot change what is downloaded.
- **A runtime of 3.7.0 is updated to today's pin, not taken for it**: llama.cpp and the other runtimes the studio downloaded before it kept records are judged by the one file of each pinned archive that tells the versions apart - its exact size read from the archive's index - so llama.cpp b11236 left on disk shows as not installed and is fetched at b11900, and ONNX Runtime 1.30.0, today's pin, is kept. Every model the studio downloads - Gemma, the karaoke's Whisper, MOSS-Music, Beat This! - comes from a pinned commit instead of a repository's latest files; the assistant's models count as installed only at their pinned size.
- **"Use models I already have" says what it left and why**, as in MiniMax Music3 Studio: besides how many files it took, it names each model file of the chosen folder it did not take - one with the studio's name but another size (another build), a GGUF of none of the studio's sets, or one it could not link or copy.
- **The writing assistant's model downloads through the HF-Mirror** when Settings - Network takes models from it, as the songs' models already did.
- **More audio decodes**: ADPCM WAV, MP1 and MP2 audio, and Matroska and CAF containers.
- **A song lost to an engine that crashed says how it ended**: the exit code the engine left (0xc0000005 an access violation, 0xc000001d an instruction the processor does not have, and others) is noted and told in words.
- **Changing the cover look no longer freezes the window** ([#150](https://github.com/timoncool/YuE2-Studio/issues/150)): switching between Pattern, Photo for the style and AI generation writes the new cover into every track, and the window read the whole library again after each track - with the replay data of every song, megabytes for a library of a few hundred - so clicks and typing stopped answering until the pass was over. The library list now leaves the replay data out (a re-render reads it from the song), and a burst of changes is read at most every two seconds.
- **Style by ear shows what it is missing** ([#168](https://github.com/timoncool/YuE2-Studio/issues/168)): the card in Settings - Models stayed red with every file of it downloaded, because the tempo model also needs ONNX Runtime and the card did not list it; the runtime is now a row of the card with its own download, and a failed install is said on the card instead of only in the service log.
- **Variations no longer stop the engine** ([#134](https://github.com/timoncool/YuE2-Studio/issues/134)): with Variations at 2 or more the graphics card failed with "CUDA error: an illegal memory access" and the song was lost - the most frequent failure of 3.7.0. The flash attention kernel that skips masked keys read past its mask for the second variation; it now reads the one mask every variation shares, and both variations render.
- **A note written while the song is being made stays** ([ACE-Step Studio #25](https://github.com/timoncool/ACE-Step-Studio/issues/25)): the note of a song still in the making was cleared when the song was done; it now goes into the finished songs and is saved with them.
- **Show JSON stays open** from song to song in the song's details, to compare their settings ([ACE-Step Studio #48](https://github.com/timoncool/ACE-Step-Studio/issues/48)).
- **The create form keeps its settings** ([YuE2 Studio #134](https://github.com/timoncool/YuE2-Studio/issues/134)): the steps, the solver, guidance, sampling and the other generation settings went back to their defaults at every start and every F5; they now stay, as the words, the length and the file format do, until "Reset to reference" puts the defaults back.
- **Change cover works in the Library** ([ACE-Step Studio #48](https://github.com/timoncool/ACE-Step-Studio/issues/48)): the button showed on the Library's songs but opened nothing; it opens the cover editor as on the create page.
- **Mel-Band RoFormer and HT-Demucs download from Settings - Models - Split into stems** ([YuE2 Studio #134](https://github.com/timoncool/YuE2-Studio/issues/134)): the card's Download button sent the engine's name, which the studio did not know ("unknown asset roformer"), so neither separator could be installed from there in 3.7.0; it now installs the engine whole on the chosen device, the bin removes that engine's files (it always removed HT-Demucs before), and the runtime downloading after HT-Demucs's model shows its progress instead of "not installed".
- **Karaoke recognition no longer closes the studio on processors without AVX-512**: the transcribe.cpp engine of 3.7.0 was built for the processor it was built on and used AVX-512 instructions, so on most Intel and older AMD processors the first karaoke stopped the studio without a word. The engine is now built for every x86-64 processor with AVX2 (Intel since 2013, AMD since 2017), as fast as before, and on a processor without AVX2 the studio says so instead of closing.
- **Karaoke recognition on the graphics card without the CUDA Toolkit**: the recogniser's CUDA build looked for cuBLAS in the program's folder and on PATH, not beside itself, so on a computer without the CUDA Toolkit it did not load (error 126) though cuBLAS had been downloaded next to it; it now finds the libraries beside it. A recogniser left by an earlier version is no longer taken for the current one: it shows as not installed until Download replaces it.
- **Describe by ear for a recording from disk** ([#131](https://github.com/timoncool/YuE2-Studio/issues/131)): on the Cover page the button showed only for a track of the library, so a recording chosen from disk had no way to get its style by ear; it now shows for either, and agents describe a file with `recording_describe_style`.
- **Make it longer carries on a song that had ended** ([#142](https://github.com/timoncool/YuE2-Studio/issues/142)): a song that stopped on its own had sung its whole score, so the seconds added had nothing to follow and came out as silence. The score is now written on first, from the section before its close, and the new part plays it in the same key, tempo and chords. A song cut short by its length still carries on under the score it has left.
- **A chord bed plays to its end** ([#141](https://github.com/timoncool/YuE2-Studio/issues/141)): the bed left the band one long rest a row ("Z4"), and over such rests the model lost count of the bars, so a song came out shorter than its bed or ran on to the length limit. The bed now writes a rest in every bar, and a 3:29 bed rendered to 3:23 on each seed we tried, where it had ranged from 3:38 to the 5:00 limit.
- **SheetSage2 counts as installed only with its MERT** ([#137](https://github.com/timoncool/YuE2-Studio/issues/137)): the transcriber reads MERT of its own precision, a file the models page never showed. With SheetSage2 on disk and its MERT missing, the page said "installed" while the engine ran without a transcriber and Cover said there was none. The page now shows it as not installed, and Download fetches the missing MERT.
- **Style by ear on macOS and Linux** ([#138](https://github.com/timoncool/YuE2-Studio/issues/138)): the captioner was only published as a Windows build, so the download said "is a Windows build and is not available on this platform yet". The Mac app and the Linux packages now carry their own captioner, built with the MIDI transcriber.
- **A new model set no longer costs the songs in the making**: when a model download finished or another set was chosen, the engine was restarted at once, and the songs queued or rendering on it were lost. The restart now waits until every song sent to the engine is back in the library, and new songs wait for the restart.
- **A lost song names the right last line**: after a run with LoRA songs the engine log held more lines than the studio read, so a song lost when the engine stopped was told the restarted engine's "[Server] Listening" instead of what the engine was doing when it stopped.
- **The training warning gives advice that works** ([#144](https://github.com/timoncool/YuE2-Studio/issues/144)): when the card's own memory was full, Fast, Balanced and Thorough were told to lower the rank, a setting those recipes do not have; they are now told to close what uses the card or to choose the As before recipe and lower its rank.
- **Training writes a step as one line**: each trained step rewrote the run's whole file with every step so far, and the training page read the whole training log twice a poll; steps now go into a file of their own a line at a time, and the log is read once.
- **Songs reach the library again with the updated engine**: the engine now sends the VAE latents of each track after its audio, and the studio refused the whole result ("unsupported engine result part content type: application/octet-stream"), so every song failed at the end. The latents are now read with their track.
- **A note written while the song is being made always reaches it** ([ACE-Step Studio #25](https://github.com/timoncool/ACE-Step-Studio/issues/25)): when the library showed the finished song before the job's answer was read, the note, the details moving to the song and the ready notice were skipped; they now happen either way, and the note shows in the details as soon as it is saved.
- **Mel-Band RoFormer says when the processor cannot run it**: its builds need AVX2, FMA, F16C and BMI2, and on an older processor the separator closed without a word; the studio now names what the processor lacks before it starts.
- **Auto does not try Vulkan on an NVIDIA card that runs CUDA**: after a failed CUDA start Auto went on to Vulkan on the same card, the path that hung NVIDIA cards until a driver reset in MiniMax Music3 Studio; such a card now goes from CUDA to the processor. An NVIDIA card neither CUDA build runs on still uses Vulkan.
- **A component downloaded by an earlier version is updated with the studio**: the engine's CUDA libraries, the writing assistant's llama.cpp, the karaoke recogniser, the stem separators and the other runtimes counted as installed as soon as files of their names were on disk, so after an update a computer kept the old build - the way ACE-Step Studio 3.7.0 kept a recogniser built with AVX-512 that closed the studio on most processors. Each download now leaves a record of the version it came from beside it, a component of another version shows as not installed and Download replaces it, removing only the files its old record lists; a download from before records is kept only when its files are those of today's version.
- **A song lost to the computer's memory says so**: an engine stopped by `std::bad_alloc` was reported as the graphics card running out of memory, and under Auto could even leave the card for the session; it now says the computer's memory (RAM) ran out, for a lost song and for a failed load, and keeps the card.
- **A score the model wrote wrong is written again**: when the planned score had no key, meter or note length, or no bars to sing ("its header has no well-formed K:"), the song stopped with that error. The engine now plans it again up to twice with the next seeds before giving up, and the same request still gives the same song ([ServeurpersoCom/yue2.cpp#21](https://github.com/ServeurpersoCom/yue2.cpp/pull/21)).
- **The Library shows a song's details beside the list**, as the create page does ([ACE-Step Studio #48](https://github.com/timoncool/ACE-Step-Studio/issues/48)): a click on a row chooses the song without playing it and the details open at the side on a wide window, over the page on a narrow one; the details button opens and closes them. In Russian the Library is called Медиатека.

### Changed

- **The update screen**, as in ACE-Step Studio and Dub Studio: the first start of this version shows what it changed in what the studio downloads - what is updated (old version → new, size), what is new, and what earlier versions left that nothing reads any more (the ONNX HT-Demucs, HOT-Step's MIDI transcriber, the ONNX Parakeet) with the space it frees - each with its reason. One button downloads it all and removes the leftovers only once what takes their place is in, and until then everything stays where it was; Later puts it off, and Settings - Models keeps it. An update downloads the new version beside the old one and removes the old files only after the new download has arrived and matched its hash. Agents read and apply it with `models_update_plan`, `models_update_apply` and `models_update_later`.
- **Stems and Audio to MIDI run on audio.cpp's server**, the one Speech Studio and Dub Studio run: HT-Demucs 6s separates from audio.cpp's own GGUF (f16, 55 MB) on CUDA, on Vulkan on AMD and Intel cards, or on the processor; MuScriptor of every size reads its folder as before and transcribes medium about twice as fast as HOT-Step's port, with the same notes, on an NVIDIA card or a Mac, and on the processor elsewhere. The studio downloads the server and its CUDA libraries itself; the server stops when its work is done, so it holds none of the card the songs need, and Stop ends a run at once. The Quality setting of the stems goes: audio.cpp cuts and joins the segments itself. The ONNX HT-Demucs and HOT-Step's transcriber of earlier versions are removed at start.
- **llama.cpp b11900** for the writing assistant; the Windows CUDA runtimes of llama.cpp are the same files and are not downloaded again. ONNX Runtime stays at 1.30.0 and now starts with its usage telemetry off - Microsoft's builds send it unless told not to - and the studio stops with the reason if it cannot turn it off.
- **Notices from Studio Hub as every studio shows them**: popups show to everyone, not only in the test channel, and wait until no song, re-render, cover or timing is being made; the sections a notice names are the hub's (create, library, news, tools, settings); a link in a popup counts as its click; the hub's news stand among the release notes by date with their picture, all their buttons and the ad label. The window's own status reads are no longer counted as features used.
- **The interface can be scaled** ([#165](https://github.com/timoncool/YuE2-Studio/issues/165)): Settings - Appearance - Interface scale draws the whole studio from 90 to 150 % and keeps it for the next start; Ctrl + plus and Ctrl + minus, or Ctrl with the mouse wheel, change it too, and Ctrl + 0 returns to 100 %.
- **Karaoke timings download as SRT too** ([#153](https://github.com/timoncool/YuE2-Studio/issues/153)): beside Download LRC in the song's details, Download SRT saves the same timings as subtitles for video editors that take SRT and not LRC (CapCut, Jianying, Premiere): each line runs until the next one starts, the last for three seconds.
- **An Analyzer look for the spectrum**, as in ACE-Step Studio ([ACE-Step Studio #58](https://github.com/timoncool/ACE-Step-Studio/issues/58)): one continuous curve over every point of the FFT, 20 Hz to 22 kHz, with a frequency scale in Hz and a level scale in dB; the Hi-Fi look now reaches 22 kHz instead of stopping at about 16.7 kHz.
- **Why a song failed is said in the studio's language**: a song refused before it is sent, lost with an engine that restarted, given up by the engine or not taken into the library is told in English, Russian, Chinese, Japanese or Korean, as the window is set - among them a song longer than the prompt and the score leave room for, and a score the model wrote that cannot be sung; the service keeps the English text for its log, the statistics and agents, with a code and fields beside it.
- **Engine updated from yue2.cpp upstream**: the latest ggml and the --device choice of the compute device; our fixes stay on top.

## 2026-10-21 — 3.7.0

### Added

- **Cleaner vocals with Mel-Band RoFormer.** Settings - Models - Stems now offers Mel-Band RoFormer voc_fv6 (Q8_0), Dub Studio's separator run by BSRoformer.cpp, beside HT-Demucs: it splits a track into the vocals and the instrumental, more cleanly, and the vocals of vocals-only songs and of training are separated with it too; on an NVIDIA card it runs on the engine's cuBLAS, elsewhere on the processor. HT-Demucs stays the default for six stems ([#108](https://github.com/timoncool/YuE2-Studio/issues/108)).
- **Sing again with new words.** A song's menu - Sing again with new words: the song comes to the create form with its score and both seeds, without the take that sang the old words, so the new lyrics are what is sung; under the lyrics the studio names every sung line whose syllable count left the old one, since such a line moves the melody. Agents count syllables with `lyrics_syllables` ([#108](https://github.com/timoncool/YuE2-Studio/issues/108)).
- **LoRA from your own folder, without a copy.** LoRA page - Add from folder: every LoRA of a folder you keep for all your programs (ComfyUI, a trainer) and its subfolders is taken in place - a folder with adapter_config.json is one LoRA, elsewhere each .safetensors is one - by a hard link, copied only from another drive, and the studio says which; agents use `lora_import_folder` ([#98](https://github.com/timoncool/YuE2-Studio/issues/98)).
- **Instrumental only.** A switch on the create page above Create: the voice of the score turns into rests before anything is sung (a score is written first when the form has none), the lyrics keep only their section tags, and the Instrumental LoRA is added to the song's LoRA list, downloaded first when it is not installed; the button then reads Create instrumental. Agents set it with `instrumental` in `create_form_set` ([#115](https://github.com/timoncool/YuE2-Studio/issues/115)).
- **A saved stem names its song's tempo and key.** A vocal or any other stem saved for a DAW is named like `Song - vocals - 109 BPM, D major.wav`, read from its score ([YuE2 Studio #108](https://github.com/timoncool/YuE2-Studio/issues/108)).
- **A unique pattern for every song.** Settings - Covers - Pattern - Unique pattern of each song: the shapes come from the song's lyrics (a song without lyrics, its title) and the colours from its style, out of the pattern style's own palette, so songs of one style share their colours and new lyrics draw new shapes; a changed style or text draws the pattern again. The choices now read Pattern, Photo, Generation; a photo for the style stays the default ([ACE-Step Studio #52](https://github.com/timoncool/ACE-Step-Studio/issues/52)).
- **Your own agent CLI writes the words.** Settings - Models - Writing assistant - Agent (CLI): Claude Code, Codex, opencode, Antigravity or Muse Code installed on this computer writes the lyrics, styles and layouts on your own account with it, one hidden run per draft, without tools; the panel shows which are installed, how to install and sign in, and tests one with a short draft. Antigravity runs only sandboxed. Agents see the CLIs with `assistant_cli_agents` ([#112](https://github.com/timoncool/YuE2-Studio/pull/112), thanks to GuilhermeAmado).
- **An agent reaches everything the studio does.** A test now holds every route of the studio to a tool or to a written reason, and the agent gains what it lacked: the network proxy and the HF-Mirror (get, set, test), access from the network, the Activity journal, the look of tracks without a cover, photo and pattern covers and photo search, video background search, a song's note, its own MIDI file, a playlist by itself, the score's MIDI export and import, instrumental, vocal octave and section marks, the graphics cards, the downloadable parts' state, and taking an ended job's card off the page.
- **Lyrics in a video clip, found where they are.** The video screen's Text tab always shows the lyrics overlay: a song without karaoke timing gets a button that makes it there, and the overlay turns on when it is done; without karaoke set up it says where to set it up. Until now the overlay section was hidden for such a song ([#118](https://github.com/timoncool/YuE2-Studio/issues/118)).
- **Fade in and fade out.** The create page's extra settings take seconds for a fade at the start and at the end of the made song, so an ending no longer stops abruptly; the studio applies them to the song it keeps, Reuse Prompt and a re-render keep them, and agents give `fade_in` and `fade_out` ([#109](https://github.com/timoncool/YuE2-Studio/issues/109)).

### Fixed

- **Karaoke on an instrumental no longer hangs on "downloading"**: the studio checked the song after fetching the recogniser and left that download's line running when it then refused the track; the song is checked first, and a finished download ends its line.
- **LoRA training on Linux** ([#126](https://github.com/timoncool/YuE2-Studio/issues/126) by boiler): the training page read the engine's CUDA build, which the Linux package never has, and refused a CUDA 13 card; it now reads what the card and driver run, and on Linux the trainer you build (`YUE_TRAIN_BIN`, `YUE_CAPTION_BIN`) links the system's CUDA. Karaoke on Linux takes the CUDA build of transcribe.cpp the same way.
- **A long step no longer gets the engine restarted** ([YuE2 Studio #129](https://github.com/timoncool/YuE2-Studio/issues/129)): an engine inside a long step does not answer its health check in time, and the studio took that for a crash and restarted it, losing the song; while a job is running and the engine process is alive, it is at work.
- **Auto stays on the graphics card after a silent engine exit** ([YuE2 Studio #127](https://github.com/timoncool/YuE2-Studio/issues/127), [#128](https://github.com/timoncool/YuE2-Studio/pull/128) by ramishi): an exit with no device error in its log (out of memory, a kill, a slow start) no longer moves Auto to the processor for the rest of the session.
- **The create form keeps its words** ([ACE-Step Studio #54](https://github.com/timoncool/ACE-Step-Studio/issues/54)): the name, the style and the lyrics stay across restarts, as the length and the file format do, until Reset clears them.
- **The training page says where to install the card runtime.** When preparation measures tempo on the processor, its notice now names the way to the graphics-card ONNX Runtime for this machine's card: Settings → Models → Optional extras → Split into stems → Graphics card on NVIDIA, Karaoke timings → Graphics card on AMD and Intel (DirectML).
- **A computer's memory as its box says it.** The studio counted RAM in 10⁹-byte gigabytes, so a 32 GB computer read 34 GB (in the issue form too); memory is now counted in 2³⁰-byte gigabytes, as video memory already was, and so are the model sets' needs.
- **A cover from a FLAC, OGG or M4A recording.** The engine reads WAV and MP3 only, and any other recording stopped at "cannot decode audio"; the studio now decodes it itself and gives the engine a WAV ([#121](https://github.com/timoncool/YuE2-Studio/issues/121), [#93](https://github.com/timoncool/YuE2-Studio/issues/93)).

### Changed

- **Karaoke's Parakeet on transcribe.cpp.** Parakeet v3 and Parakeet Ultra run as GGUF Q8_0 on transcribe.cpp, the engine Dub Studio and Speech Studio use, instead of ONNX Runtime: its CUDA build on an NVIDIA card with a CUDA 13 driver, its Vulkan build on any other card (and on Linux without the system's CUDA 13 cuBLAS), its processor build without a card; on a Mac Metal on Apple silicon and the processor on Intel. The files have Dub Studio's names and folders, so one program's copy serves the others, and the model leaves the graphics card when a recognition ends, so the card is the music engine's again. A saved fp32 choice becomes Parakeet v3 (full weights are not kept), and the ONNX Parakeet folders of 3.6 and earlier are removed at start.
- **Whisper on the graphics card keeps int8 weights** (int8_float16) instead of float16.
- **Audio to MIDI and the training pack are one download.** The MIDI transcriber, the trainer and the captioner come from one HOT-Step build that shares its graphics-card libraries, in one folder: 133 MB once instead of 126 and 132 MB apart, and installing either one installs both ([YuE2 Studio #104](https://github.com/timoncool/YuE2-Studio/issues/104)).

## 3.6.1 — not released on its own; its fixes are part of 3.7.0

### Fixed

- **An agent sees the window as you do.** Through the studio's MCP it now sees folded sections (open or closed) and the options of open lists, reads a field by its own label rather than by the options inside it, and a field it fills while the window is in the background saves as after your own click.
- **LoRA preparation says which song does not fit and why.** A song longer than the model's 24576 tokens (prompt, score and audio together) passed preparation and failed at its end with "prepared manifest failed reader roundtrip"; the trainer now names the song and its sizes, and a roundtrip failure carries the reader's reason ([#110](https://github.com/timoncool/YuE2-Studio/issues/110)).
- **Karaoke and stems on a Mac.** The Mac build downloaded the Windows ONNX Runtime and could not load it; it now takes Microsoft's macOS arm64 build, runs Parakeet and the separator on the processor, and the app may load that library (it is signed by Microsoft, not with the app). Whisper and the CUDA and DirectML parts are not offered on a Mac, where they cannot run ([#105](https://github.com/timoncool/YuE2-Studio/issues/105)).
- **A download Hugging Face does not answer says why and what to do.** The error kept only "error sending request" and lost the cause (the name not found, a timeout, a refused connection); it now names it, and when the model comes from Hugging Face it points to Settings - Network, which can take models from the HF-Mirror instead.
- **Karaoke that cannot start says why and ends.** An ONNX Runtime the studio could not load stopped the recogniser with a panic: the karaoke stayed "running" for ever and every later try answered "Mutex poisoned" until a restart. The library is now loaded before any work and a failure is that work's error, with the library's path; a recogniser that stops ends its karaoke as failed ([#106](https://github.com/timoncool/YuE2-Studio/issues/106)).
- **The song details button is back on wide windows.** 3.6.0 hid it on the create page whenever the details could sit beside the list; it shows at every width again ([#99](https://github.com/timoncool/YuE2-Studio/issues/99)).
- **The Linux AppImage starts its engine again.** The AppImage put the MIDI tool's older ggml library on the path every program inherits, so the engine loaded it instead of its own and stopped at once ("undefined symbol: ggml_prec_set_acc"). Every program the studio starts now loads the libraries beside it first: the engine, the writing assistant, the trainer, Audio to MIDI and the karaoke recogniser ([#97](https://github.com/timoncool/YuE2-Studio/issues/97)). Thanks to angelside, who found the cause.
- **The writing assistant works on Linux.** It downloaded the Windows builds of llama.cpp and could never start; it now takes llama.cpp's Linux builds for the processor, Vulkan and CUDA ([#114](https://github.com/timoncool/YuE2-Studio/issues/114)).

## 2026-10-16 — 3.6.0

### Added

- **A library as roomy as a music service's.** Compact rows show three times as many songs; the tabs count their songs and the filters say how many of how many are shown. Filters pick where songs came from (made in the studio, imported, stems and processing) and those with karaoke or processed versions; Recently played joins the orders. A right click on a song opens its menu, which gains Play next and Add to queue. In a selection the bar stays on screen while the list scrolls, Shift+click takes a range, Ctrl+A every song and Esc leaves it, and the chosen songs can be liked, queued or downloaded at once. Liked and Playlists say what to do when they are empty.
- **Playlists to keep and play.** A playlist's name and description change in place, its songs move by dragging or Alt+Up/Down, and songs are found and added on its own page; a song taken out comes back with Undo. Its picture is the covers of its songs, the same on its card and its page; the song playing is marked; Shuffle plays it in a mixed order, and a card has Play on its cover and a menu on a right click. Adding songs says which playlist they went to and which were there already. Back from a playlist returns to the Playlists tab. Agents queue songs with `player_queue_add`; `playlist_update` takes the whole order.
- **Winamp plays the studio's playlists.** The studio's shuffle and repeat go into Winamp and come back; LOAD LIST and ADD open the studio's playlists, the liked songs and the whole library, and SAVE LIST writes over the playlist the music came from or into a new one.
- **A track's file carries how it was made.** Besides the title, lyrics and cover, the file now holds the song's note as its comment, its whole style, and its generation settings (a JSON document of seed, models and options, without paths on your computer) in fields of the studios' own. Importing such a file back fills Reuse Prompt with what it was made with, in the studio that made it; any file brings its title and lyrics, and the library says how many files came in. Tracks made before get theirs at the first start. Settings › Privacy turn it off, and every stored track is tagged again ([ACE-Step-Studio #50](https://github.com/timoncool/ACE-Step-Studio/issues/50)).
- **Several tracks to MIDI at once.** Choose songs in the library or on the create page and press To MIDI: they are turned into MIDI one after another, Studio tools shows which track it is on and how many are done, Stop ends the set, and a track that fails is named while the rest go on. Agents give `song_ids` to `midi_transcribe` ([YuE2-Studio #94](https://github.com/timoncool/YuE2-Studio/issues/94)).
- **Sorting out a long list of songs.** A song's row has a Delete button beside Add to playlist, and the chosen songs of a selection go to a playlist in one go. The Library's All and Liked tabs have the create page's search, filters, selection, Like, Add to playlist and Delete, and a details button that opens the song's details over the page and closes them again ([ACE-Step-Studio #48](https://github.com/timoncool/ACE-Step-Studio/issues/48)).
- **OGG and FLAC songs can be imported into the library**, beside MP3 and WAV, and turned into MIDI, stems or karaoke like any other ([YuE2-Studio #94](https://github.com/timoncool/YuE2-Studio/issues/94)).
- **The version in the window title and under the studio's name**, with "Portable" on a portable copy, so a copy waiting to be updated tells which it is ([ACE-Step-Studio #40](https://github.com/timoncool/ACE-Step-Studio/issues/40)).
- **Every settings slider takes a typed number.** The value beside a slider is a field: type `0.40` for a LoRA's strength, `2:10` for a duration or `6` for an equalizer band, and Enter sets it, kept within the slider's range (a comma works as the decimal point, Escape puts the value back). On LoRA strength, the creation settings, audio processing, the equalizer, the Winamp size and the video clip editor.
- **Any engine launch flag and a hand-built engine.** Settings - Local engine take one line of extra flags for the engine, split as a shell would and placed after the studio's own, and an engine executable of your own build, picked in a file dialog and checked when saved. The restart button saves the page first and starts what it shows; an old engine still holding the port is waited for and refused instead of being reused with its old flags. Ported from [@wuyue92tree](https://github.com/wuyue92tree)'s change to ACE-Step Studio ([ACE-Step-Studio #46](https://github.com/timoncool/ACE-Step-Studio/pull/46)).

### Fixed

- **"Instrumental" is not a genre.** A style that starts with "instrumental" no longer files the track under the genre Instrumental in its file; the next phrase that names music is taken, or none.
- **An installation under Program Files downloads the engine's libraries.** The studio put NVIDIA's cuBLAS beside the engine, where Windows lets no program write without administrator rights, so an installed studio stood at "Downloading the engine library - 0%" for ever. Where the engine's folder cannot be written, the libraries now go to the studio's data folder and the engine finds them there ([ACE-Step-Studio #49](https://github.com/timoncool/ACE-Step-Studio/issues/49)).
- **Stems, karaoke and Audio to MIDI work on Linux.** The studio downloaded the Windows builds of their runtimes on Linux and stopped there without a word. It now takes Microsoft's and NVIDIA's Linux builds of the same versions - ONNX Runtime with CUDA, cuBLAS, cuDNN, cuFFT, cuRAND - and Purfview's Linux Whisper, runs the separator and recogniser on an NVIDIA card there, and the Linux packages carry the MIDI transcriber ([YuE2-Studio #95](https://github.com/timoncool/YuE2-Studio/issues/95)).
- **A download that cannot start says so in its panel** instead of leaving the bar where it was.
- **"Use models I already have" takes only the models in the folder.** A file was taken for every model of the same size, and many models of one architecture weigh the same to the byte, so four files came in as a dozen models with the wrong weights under their names; a file under another name is now taken only when its contents are that model's ([ACE-Step-Studio #40](https://github.com/timoncool/ACE-Step-Studio/issues/40)).
- **One version everywhere.** The studio's version is written in `desktop/src-tauri/tauri.conf.json` alone: the window, the service, the interface, the project page and the release all read it there, and the studio names itself to the sites it asks with it instead of 0.1.0.
- **An agent's command to a closed window fails at once.** A window that closed without saying so kept the agent waiting the full five minutes for its answer; the command now fails as the window goes, and the next call reaches another open window or says none is open.
- **The window gets the microphone it asks for.** WebView2 left the request with no answer and no prompt, so recording in the audio editor waited for ever; the studio's own page is now allowed the microphone, nothing else is.
- **A song lost to the graphics card names the error.** "The graphics card failed" now carries the engine's own CUDA or Vulkan error, and a song lost to an engine that ended without a word says the last thing it wrote, so the cause can be told apart instead of guessed.
- **A recording that will not play says why.** The player of a training song or a stem turned to Pause and stayed silent when the window could not play the file; it now stays on Play and names the reason the window gave ([YuE2-Studio #75](https://github.com/timoncool/YuE2-Studio/issues/75)).
- **Training songs play in the window.** The player of a song in a training set asked the window's own page for the recording instead of the studio, so in the window it stayed silent while a browser played it ([YuE2-Studio #75](https://github.com/timoncool/YuE2-Studio/issues/75)).
- **Access from the network can be turned on from a browser on the studio's own computer**, not only in its window, so a studio on a server without a desktop is reachable; and access turned on by hand in `studio-settings.json` without a key now gets one when the studio starts instead of staying on this computer only ([ACE-Step-Studio #47](https://github.com/timoncool/ACE-Step-Studio/issues/47)).
- **Downloads that stall come back.** A connection that stopped sending held the download at the same byte for ever, with no error and no retry - most visibly the engine libraries from NVIDIA, which kept the start screen at "Downloading the engine libraries - 0%" ([YuE2-Studio #92](https://github.com/timoncool/YuE2-Studio/issues/92)). A silent connection is now dropped after a minute and the piece is fetched again, and a server that refuses a file says so at once instead of after two minutes of retries.
- **Only the studio's own window and agents can use it.** A web page open in your browser could send requests to the studio on this computer, and the studio answered any site; it now refuses pages of other sites, and requests that reach it under another site's name (DNS rebinding), as Dub Studio does.

## 2026-10-14 — 3.5.5

### Fixed

- **The engine starts on macOS.** The bundled engine looked for its libraries in the build machine's folder and never started on a Mac; it now finds them beside itself ([#79](https://github.com/timoncool/YuE2-Studio/issues/79), [#81](https://github.com/timoncool/YuE2-Studio/issues/81)).
- **The writing assistant works with OpenRouter.** Saving the model put the assistant back on a local provider, so with a key and a model chosen it still said "not installed" and the wand opened the setup page; the choice now stays, and the providers page no longer undoes a choice made on the models page ([#80](https://github.com/timoncool/YuE2-Studio/issues/80)).
- **Imported tracks show their real length.** An MP3 was measured as if it were 128 kbps, so a 320 kbps file showed two and a half times longer in the library than in the player; files are measured from their own headers, and tracks already imported are measured again on start ([#77](https://github.com/timoncool/YuE2-Studio/issues/77)).
- **The song details button stays on wide windows.** It disappeared above a certain width, and once the side panel was closed nothing opened it again ([#84](https://github.com/timoncool/YuE2-Studio/issues/84)).
- **The audio editor keeps the sample rate.** It decoded every file at 44.1 kHz, so a 48 kHz song was resampled on opening and saved at 44.1 kHz; it now opens and saves at the file's own rate ([#83](https://github.com/timoncool/YuE2-Studio/issues/83)).
- **"Without stopping" stops when a song fails.** It sent the same request again and again although each one failed - a deleted source song, a card out of memory - hundreds of times in a row; a failed song now switches it off and says why.
- **A failed song names the cause.** The message showed the engine's last complaint, such as "encode failed", instead of the first one that caused it.
- **Models not downloaded yet are called that.** "Not installed" next to a model read as a broken installation.

## 2026-10-11 — 3.5.4

### Fixed

- **Exports from the MIDI editor are saved.** Export WAV, Export MP3 and the MIDI download filled their progress bar and then did nothing in the desktop window; they now open the Save dialog, named after the song ([YuE2-Studio #71](https://github.com/timoncool/YuE2-Studio/issues/71), [#72](https://github.com/timoncool/YuE2-Studio/issues/72)).
- **Playback on Linux.** The AppImage now carries the GStreamer libraries and plugins the window plays sound with, so playing a song no longer crashes on systems such as Arch ([YuE2-Studio #70](https://github.com/timoncool/YuE2-Studio/issues/70)).
- **Songs stay on this computer's engine.** Settings from an older build could switch song-making to OpenRouter with no model chosen, and every song then stopped with "OpenRouter music requires a selected model"; songs go back to the studio's own engine, and OpenRouter can be picked for songs only once its catalog offers a model ([ACE-Step-Studio #37](https://github.com/timoncool/ACE-Step-Studio/issues/37)).
- **The "Classical Music" example was a Christian song** (mislabelled at its source); it is now named for what it is ([#69](https://github.com/timoncool/YuE2-Studio/issues/69)).
- **Lengthening a song adds the seconds asked for.** Re-render with "+N s" carried on from a track that had already ended, so the model often ended it again at once and the song stayed as long as before; the end now waits until most of the added seconds are composed.
- **A context of 4096 no longer stops every song.** The score alone can take 4096 tokens, so the engine setting now starts at 8192, and a smaller saved value is raised.

## 2026-10-10 — 3.5.3

### Fixed

- **Works offline.** With the computer offline the window held every read and change until a network came back:
  a song stood at its last stage although it was made, and a song deleted came back when Wi-Fi was turned on.
  The window talks to the studio on this computer and no longer waits for a network.
- **Describe by ear hears FLAC songs.** Since songs are kept as FLAC the captioner refused them; they are handed
  to it as WAV.
- **WAV (32-bit float)** is offered again beside FLAC and MP3, in the form and in Re-render.
- **Audio to MIDI keeps the size you pick.** The models page put the choice back to medium while a download ran,
  so small and large looked impossible to get; the largest size downloaded is now the one offered first, on the
  models page and on the tools page.
- **Send to the form keeps the sound seed.** A song with only the sound seed fixed came to the form with the random
  switch on, which threw that seed away; the switch is now off whenever the song has a seed.
- **Linux packages run on Ubuntu 22.04** and other systems with glibc 2.35 or newer; they needed 2.38.

## 2026-10-10 — 3.5.2

### Added

- **News strips.** Short notes from the author appear as a strip across the top of the window, in the
  window's language, with a link and a close button; the first one is about Dub Studio, the video dubbing
  studio. A closed strip does not come back.

### Fixed

- **Quitting and updating ask in the studio's own language.** The question about quitting while a song is
  being made and the update offer are now part of the window, in its language and look, instead of English
  system dialogs. Closing the window a second time still quits if the window stopped answering.
- **The day's statistics leave when the studio closes** instead of at its next start.
- **A model set put together by hand is counted as it was when the song started**, not as it was when the
  song ended.
- **Statistics count finished songs by job** as well as by song, so the share of failed songs compares
  like with like.
- **Linux builds are marked experimental** in the README, with an invitation to Linux users to polish them
  and send pull requests.

## 2026-10-10 — 3.5.1

### Added

- **The score plays with a cursor.** Listen marks the notes sounding and moves a cursor along the
  staves, scrolling to each new line; clicking a note plays from it (during playback it jumps there),
  the slider beside Listen moves through the score with its time shown, and Pause keeps the place.
- **Edit beside Listen** opens the score in the note editor (piano roll, tracks, chords): in the
  create form the form's score, on a song its score, sent to the form first.

### Fixed

- **Statistics name the model set in full.** A set put together by hand is reported by its parts, and a day
  on several sets reports each with its number of songs; a failed song reports its reason, with paths, names,
  links and quoted text cut out on this computer before it leaves.
- An agent's Play no longer takes over the player while you listen: a song you are playing is neither
  switched nor paused, and the agent is told you are listening.
- Quitting while the service is slow to answer stops the songs being made as asked, instead of letting
  them start again at the next launch.
- A like is a heart everywhere: the song list and its Liked filter showed a thumbs-up while the
  player and the side panel showed a heart.
- On the start screen the statistics checkbox, its label and What is sent stand on one line in one
  colour; the link sat lower than the label and in another colour.
- The notes of 3.4.0 and 3.5.0 in News are laid out like the earlier ones: a summary line, then each
  change as a point with its name in bold.

## 2026-10-08 — 3.5.0

### Added

- **Anonymous statistics and news from the hub.** The start screen and Settings - Anonymous statistics have
  a checkbox, on by default, with which the studio sends once a day how many songs were made, failed or
  were cancelled, the model set used, its version, the OS and the class of the graphics card - never lyrics,
  prompts, audio or file names; What is sent shows the report, and DO_NOT_TRACK=1 or STUDIO_TELEMETRY=0
  turns it off entirely. News from the author arrive without an update, on top of the bundled ones.
- **AB2 solver** under Advanced (Riff's idea): one network evaluation a step instead of two, so the
  sound stage takes about half the time at the same steps; in our comparison it sounded the same
  as midpoint, which stays the default. Agents set `solver`.
- **A song from a seed** (Mothersuperior's hum-to-song, its first part): with "Continue the song from
  this score" the score is only an opening - hummed and transcribed, played on the keyboard or
  written - and the model writes the rest of the song on from it in its key and meter, the opening
  coming back as a hook; silent bars at its end are dropped. Agents set `abc_continue`.
- **Chord bed** beside Compose the score (gary4juce's idea): a score of chords over rests written at
  once in the tempo, key and meter you set, one section per lyrics tag, verses, choruses and a bridge
  each on a common progression of their own; the model writes the melody and the arrangement over
  it and the planning step is skipped. Agents use `score_chord_bed`.
- **Chords and sections** under Advanced, for the score the model writes (Yeufonic's idea): chord
  variety lowers the chords heard among the recent changes so the song stops looping one
  progression, a section can be kept from opening the way the one before did, chords outside
  the key can be favoured, and the score can be held to the lyrics' sections in their order.
  Off by default; agents set it with `harmony` on song_create and score_compose.
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
- **The model set fits the computer's memory, not only the card's.** On integrated graphics or the
  processor the weights live in RAM, so a laptop is now offered the set its memory holds, and every
  set that needs more memory than the computer has says so on the start screen instead of stalling
  half loaded. The studio no longer recommends the unquantised BF16 set: Q8_0 is near lossless at
  half the memory, and the BF16 set stays in the list.
- **Match sections to the score** for a cover (after HOT-Step's Cover Studio): the source recording is
  heard by the karaoke recogniser and each lyric block is tagged with the score section it is sung in.
  Words never change; a chorus written once is copied into later chorus sections, sections without
  voice get an empty tag, blocks not heard clearly keep their place and are listed. The proposal is
  shown before and after and applied only on Apply. Also an MCP tool, `score_match_sections`.
- **Voice an octave down or up**: the score view moves only the vocal line by an octave, the band,
  chords and key stay. The written register decides who sings, so an octave down brings in a man; and
  a vocal line whose middle leaves C4–A#5, where the model writes the voice itself, is flagged.
- **Russian stress**: a stress mark (U+0301, Alt+0769) after a vowel puts the sung stress on it; the
  lyrics hint says so.
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

- Long songs no longer fail now and then at the sound stage with a CUDA illegal memory access: the
  engine's attention keys are padded to the flash attention kernels' tile (the sound is unchanged,
  and that stage got faster).
- A cover's melody score no longer shrinks to a quarter of its size in the score view and its PDF:
  the voice's multi-bar rests are drawn bar by bar.
- A score the model wrote as garbage (no key, no meter, colons in the notes; LoRA strengths far past
  their limits do this) stops the song in seconds with the reason, instead of minutes of singing
  from it.
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
