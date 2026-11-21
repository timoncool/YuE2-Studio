# YuE2 Studio showcase

[Back to YuE2 Studio](../../README.md)

![YuE2 Studio](../../docs/screenshots/en-01-create.png)

## Screenshots

![MIDI editor with two tracks, voice and piano notes in the piano roll](../../docs/screenshots/en-16-midi-editor.png)

![The chord bed: chords over rests in your tempo, key and meter](../../docs/screenshots/en-17-chord-bed.png)

| | |
|---|---|
| ![The equalizer and MilkDrop](../../docs/screenshots/en-13-listen.png) | ![Winamp mode with the classic player, equalizer and playlist](../../docs/screenshots/en-14-winamp.png) |
| The equalizer with its curve and MilkDrop over the studio, as the song plays | Winamp mode: the classic player, equalizer and playlist in the original skin |
| ![The score](../../docs/screenshots/en-02-score.png) | ![A finished track](../../docs/screenshots/en-03-track.png) |
| The score as sheet music and as ABC text — melody + chords, melody only, or none | A finished track — its lyrics and the score it was sung from, ready to reuse |
| ![Cover mode](../../docs/screenshots/en-04-cover.png) | ![Models](../../docs/screenshots/en-05-models.png) |
| Cover mode — pick a recording, SheetSage2 writes its melody down | Model sets — one quantisation per role, what is on disk, switch in one click |
| ![Any track to MIDI](../../docs/screenshots/en-09-midi.png) | ![Choose the base model and review LoRA training settings](../../docs/screenshots/en-08-training.png) |
| Any track to MIDI — a piano roll of every instrument, played against the original, with mute and solo | Choose the training base, review the three-song dataset, trigger word and recipe before training |
| ![A training dataset with three demo songs](../../docs/screenshots/en-07-dataset.png) | ![The LoRA catalogue of styles, artists and sounds, with author credits and download sizes](../../docs/screenshots/en-06-lora.png) |
| A training dataset with three demo songs, ready for training | The LoRA catalogue — styles, artists and sounds, with author credits and download sizes |
| ![Audio processing](../../docs/screenshots/en-10-processing.png) | ![Agent over MCP](../../docs/screenshots/en-11-agent.png) |
| Audio processing — noise reduction, the Spectral Lifter, a vocal naturaliser, VST3, mastering | An agent over MCP — the address and the lines to paste into Claude Code or any client |

### Studio 5.0

| | |
|---|---|
| ![Choose a model and music engine in Create](../../docs/screenshots/en-18-music-engines.png) | ![Engines and updates settings](../../docs/screenshots/en-19-engines-updates.png) |
| Create panel model and engine selectors, with a link to installable engines. [Models and runtimes](models-and-runtimes.md#models) | Install and manage engine components and model profiles. [Models and runtimes](models-and-runtimes.md#what-runs-where) |
| ![Songbook chord sheet](../../docs/screenshots/en-20-songbook.png) | ![Compare takes in a playlist](../../docs/screenshots/en-21-take-comparison.png) |
| Lyrics and chord names with guitar shapes, transpose and capo controls. [Scores and Songbook](creating-music.md#score-and-songbook) | A/B comparison keeps playback positions aligned and lets you mark a preferred take. [Compare takes](creating-music.md#compare-takes) |
| ![Two-deck DJ mode](../../docs/screenshots/en-23-dj-mode.png) | ![Library with imported tracks](../../docs/screenshots/en-library.webp) |
| Two decks with cue, sync, pitch and mix controls. [DJ mode](creating-music.md#dj-mode) | Imported tracks in the Library. [Bring an existing library into YuE2](getting-started.md#bring-your-ace-step-or-minimax-library-into-yue2) |
| ![ACE-Step or MiniMax library import preview](../../docs/screenshots/en-24-library-import.png) | ![Full-screen player](../../docs/screenshots/en-player.webp) |
| Source library, contents, required disk space and the **Copy library into YuE2** action. [Import an existing library](getting-started.md#bring-your-ace-step-or-minimax-library-into-yue2) | Full-screen playback controls and song artwork. [The player](creating-music.md#what-you-can-do) |
| ![Before and after audio processing](../../docs/screenshots/en-normalise.webp) | |
| Listen to a processed result against the original, then save it as a new track. [Audio processing](creating-music.md#what-you-can-do) | |
| ![RVC voice catalog](../../docs/screenshots/en-22-rvc-voices.png) | ![RVC voice training](../../docs/screenshots/en-25-rvc-training.png) |
| Browse curated RVC voices or import a voice checkpoint and optional retrieval index. [RVC voice catalog](creating-music.md#rvc-voice-catalog) | Train an RVC voice from a prepared dataset on NVIDIA CUDA. [RVC training guide](../../docs/rvc-burn-runtime.md) |
| ![Example LoRA training result](../../docs/screenshots/en-26-lora-training.png) | |
| Demonstration run with a loss curve, checkpoint comparison and a **Step 50 · To LoRA** action. [LoRA training](creating-music.md#lora-training) | |

The same screens in the language you read: [Русский](https://timoncool.github.io/YuE2-Studio/ru.html),
[中文](https://timoncool.github.io/YuE2-Studio/zh.html), [日本語](https://timoncool.github.io/YuE2-Studio/ja.html),
[한국어](https://timoncool.github.io/YuE2-Studio/ko.html),
[Português](https://timoncool.github.io/YuE2-Studio/pt.html) — on the project page, or in
[docs/screenshots](../../docs/screenshots).

## Samples

Made in the released build on an RTX 4090 with the recommended BF16 set, nothing edited
afterwards. Listen in the browser on the [project page](https://timoncool.github.io/YuE2-Studio/#samples)
or download the MP3s from [docs/samples](../../docs/samples).

| Song | How it was made | Style |
|---|---|---|
| [Houseplant Party](../../docs/samples/houseplant-party.mp3) | English funk disco | funk disco, slap bass, wah guitar, brass, 118 BPM |
| [My Truck Talks Back](../../docs/samples/my-truck-talks-back.mp3) | Country | country, male baritone, pedal steel, fiddle, 96 BPM |
| [La Siesta](../../docs/samples/la-siesta.mp3) | Latin pop in Spanish | latin pop, nylon guitar, congas, piano montuno, 102 BPM |
| [外卖小哥](../../docs/samples/delivery-rider.mp3) | Mandarin city pop | city pop, electric piano, funky bass, 108 BPM |
| [月曜日のサムライ](../../docs/samples/monday-samurai.mp3) | Japanese rock, anime-opening style | j-rock, female voice, distorted guitars, 168 BPM |
| [Дача, лето, комары](../../docs/samples/dacha-leto-komary.mp3) | A full song from a style and lyrics | folk pop, warm male voice, accordion, 104 BPM |
| [Дача, лето, комары — джаз](../../docs/samples/dacha-jazz-cover.mp3) | Cover: SheetSage2 wrote down the melody of the song above, YuE2 sang it as jazz | vintage jazz swing, sultry female crooner, muted trumpet |
| [Понедельник](../../docs/samples/ponedelnik.mp3) | Blues rock, raspy voice | blues rock, overdriven guitar, hammond organ, 96 BPM |
| [Бабушка на дискотеке](../../docs/samples/babushka-na-diskoteke.mp3) | Re-rendered from its saved audio codes with 64 solver steps | disco pop, funky bass, string section, 120 BPM |
| [Борщ на орбите](../../docs/samples/borshch-na-orbite.mp3) | Ska punk | ska punk, brass section, offbeat guitar, 150 BPM |
| [Кот-программист](../../docs/samples/kot-programmist.mp3) | Synthwave pop, the second take | synthwave pop, analog synths, drum machine, 112 BPM |
| [Ночной вор Барсик](../../docs/samples/barsik.mp3) | Style and lyrics written by the built-in assistant from a one-line idea | pop-punk, distorted guitars, 185 BPM |

## Community reviews

Video reviews and tutorials from the community (the description links this repo):

<table>
  <tr>
    <td width="33%" align="center" valign="top">
      <a href="https://www.youtube.com/watch?v=6OnV_80f6ko"><img src="../../docs/reviews/r1.jpg" width="100%"></a><br/>
      <a href="https://www.youtube.com/watch?v=6OnV_80f6ko"><b>AI Controversy</b></a> · EN<br/>
      <sub>Ditch Suno: This FREE AI Makes Full Songs on Your PC</sub>
    </td>
    <td width="33%" align="center" valign="top">
      <a href="https://www.youtube.com/watch?v=JrNTvpoh318"><img src="../../docs/reviews/r2.jpg" width="100%"></a><br/>
      <a href="https://www.youtube.com/watch?v=JrNTvpoh318"><b>ArtSonica</b></a> · ID<br/>
      <sub>Music AI GRATIS OFFLINE / NO INTERNET</sub>
    </td>
    <td width="33%" align="center" valign="top">
      <a href="https://www.youtube.com/watch?v=XxkGZU89FII"><img src="../../docs/reviews/r3.jpg" width="100%"></a><br/>
      <a href="https://www.youtube.com/watch?v=XxkGZU89FII"><b>AI VINCI</b></a> · EN<br/>
      <sub>The Suno Killer is Here: it's called YuE2 Studio</sub>
    </td>
  </tr>
  <tr>
    <td width="33%" align="center" valign="top">
      <a href="https://www.youtube.com/watch?v=kpaV_1jyO3E"><img src="../../docs/reviews/r4.jpg" width="100%"></a><br/>
      <a href="https://www.youtube.com/watch?v=kpaV_1jyO3E"><b>Mamedov Info</b></a> · RU<br/>
      <sub>YuE2-Studio: лучшая локальная замена Suno</sub>
    </td>
    <td width="33%" align="center" valign="top">
      <a href="https://www.youtube.com/watch?v=JPUU9zB7yio"><img src="../../docs/reviews/r5.jpg" width="100%"></a><br/>
      <a href="https://www.youtube.com/watch?v=JPUU9zB7yio"><b>ЮЛАВИ Studio</b></a> · RU<br/>
      <sub>Забудь про Suno? Тестируем YuE2 Studio локально</sub>
    </td>
    <td width="33%" align="center" valign="top">
      <a href="https://www.youtube.com/watch?v=-TjHGgxKwBU"><img src="../../docs/reviews/r6.jpg" width="100%"></a><br/>
      <a href="https://www.youtube.com/watch?v=-TjHGgxKwBU"><b>Wizzdome-Music</b></a> · EN<br/>
      <sub>YuE2 Studio On Your Own PC</sub>
    </td>
  </tr>
</table>

More tutorials and reviews:

- [Dream Travel — YuE2 Studio для генерации музыки прямо на компе](https://www.youtube.com/watch?v=GmrstqfzC9k) · RU
- [Luki AI Lab — NEW YuE2: a FREE alternative for your PC](https://www.youtube.com/watch?v=c5kwPNpkAO8) · EN
- [Skillo Labs — Você já pode criar músicas com IA sem internet](https://www.youtube.com/watch?v=8AIaqTRrdPc) · PT
- [아빠너구리 TV — $0/Month AI Music with YuE2 Studio](https://www.youtube.com/watch?v=08yKZTgMK_k) · KO
- [Frihandhika — Tutorial YuE2 Studio di Windows](https://www.youtube.com/watch?v=dBetNSxbo-A) · ID
- [Run It Local — YuE2 Studio: FREE AI Songs on AMD (Beats ComfyUI)](https://www.youtube.com/watch?v=BtlPRhbLNoM) · EN
- [Franck D Digital AI — YuE2 Studio Portable (tutoriel français)](https://www.youtube.com/watch?v=Br6ABTJflPg) · FR
- [EZTV — YUE2 Studio 教學](https://www.youtube.com/watch?v=EFv_zOXyn5E) · ZH

**Made a review or tutorial?** Put a link to the project in the description — we feature the best ones on the [site](https://timoncool.github.io/YuE2-Studio/) and here. Send yours through [Issues](https://github.com/timoncool/YuE2-Studio/issues).

Next: [get started](getting-started.md) · [create and edit music](creating-music.md) ·
[get help](community.md#report-a-problem).
