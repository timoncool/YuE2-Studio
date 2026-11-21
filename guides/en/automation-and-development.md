# Automation and development

[Back to YuE2 Studio](../../README.md)

## For AI agents

Given this repository, an agent can set everything up and drive the studio by itself:

1. Install the studio from the [latest release](https://github.com/timoncool/YuE2-Studio/releases/latest) and start it,
   with `--headless` to keep its window off the user's screen (see
   [Drive it from an agent](#drive-it-from-an-agent-mcp)).
2. Connect to its MCP server at `http://127.0.0.1:8791/mcp`:
   `claude mcp add --transport http yue2-studio http://127.0.0.1:8791/mcp`
3. Read the skill it serves (resource `studio://skill`, prompt `studio`), the same text as
   [docs/mcp-skill.md](../../docs/mcp-skill.md), and start with the tool `studio_status`.

[llms.txt](../../llms.txt) says the same for tools that look for it. To keep the skill in Claude
Code, save [docs/mcp-skill.md](../../docs/mcp-skill.md) as `~/.claude/skills/yue2-studio/SKILL.md`.

## Drive it from an agent (MCP)

While the studio is open it serves MCP at `http://127.0.0.1:8791/mcp`: an agent such as
Claude Code, Claude Desktop or Cursor does everything the page does, through the same code -
songs and scores, the library, covers, stems, MIDI, karaoke, processing, video clips, the player,
LoRA, and a LoRA from a folder of songs end to end - and sees and works the window itself:
a screenshot, its controls, clicks and typing. 162 tools, grouped by area. The model's
writing rules and official examples come with the server, so the agent writes the styles,
lyrics and lyric layouts itself instead of the studio's small assistant.

With **Agent (MCP)** chosen as the writing assistant (Settings, Models), the connected agent
also answers the studio's own write buttons and dataset preparation. Settings, **Agent
(MCP)** shows whether an agent is connected and what to paste into the client.

```bash
claude mcp add --transport http yue2-studio http://127.0.0.1:8791/mcp
```

**Without a window.** An agent that finds the studio closed starts it with `--headless`: the
studio runs as usual with its window hidden, a tray icon the only sign of it, and stops by
itself after ten minutes with no agent calls and nothing at work (`YUE_STUDIO_IDLE_SHUTDOWN_S`
sets other seconds). Opening the studio the usual way brings its window up in the same
session. `--stop` ends it, unless its window is open or something is at work. The installed
executable is `%LOCALAPPDATA%\YuE2 Studio\YuE2-Studio.exe` on Windows (or the folder chosen
when installing), `/Applications/YuE2 Studio.app/Contents/MacOS/YuE2-Studio` on a Mac and
`/usr/bin/YuE2-Studio` from the .deb.

```powershell
Start-Process "$env:LOCALAPPDATA\YuE2 Studio\YuE2-Studio.exe" -ArgumentList '--headless' -WindowStyle Hidden
```

[docs/mcp-skill.md](../../docs/mcp-skill.md) is the skill an agent reads: every tool, what the
model expects, and step-by-step recipes.

## Architecture

```text
React UI ─┐
          ├─ YuE2-Studio.exe   (Tauri window + Rust/Axum service on 127.0.0.1:8791)
Rust axum ┘        │
                   ├─ yue2.cpp `yue-server`   (C++/CUDA, GGUF, 127.0.0.1:18087)
                   ├─ music-train.exe         (HOT-Step ace-train, LoRA training, optional)
                   └─ vst-host.exe            (HOT-Step VST3 host, a process of its own)
```

The service is compiled into the desktop binary. It supervises the engine process, restarts
it when you switch model sets, and imports finished songs itself, so a result is never lost
if the window was reloaded or closed mid-generation. Progress is read from the engine's own
log: score, audio codes, acoustic rendering, decoding.

## Building from source

```powershell
npm --prefix app install
npm --prefix desktop install
cargo test --workspace
npm --prefix app test
```

Developing the UI against a running service:

```powershell
cargo run -p music-server           # service on 127.0.0.1:8791
npm --prefix app run dev            # UI on 127.0.0.1:3791
```

The engine: `scripts/build-yue-runtime.ps1 -RuntimeBackend all` builds the pinned yue2.cpp
commit (`engines/yue2-cpp-source.json`) with runtime-loaded CUDA, Vulkan and CPU backends,
using CUDA 13, the Vulkan SDK, MSVC and Ninja. `scripts/build-release.ps1 -Version X.Y.Z` produces the NSIS installer, the
portable archive and the signed `latest.json` for the updater; it reads the signing key from
`TAURI_SIGNING_PRIVATE_KEY` or `%USERPROFILE%\.tauri\yue2-studio.key`. Model weights are
never part of a release.

Linux packages are built on the release machine in an Ubuntu 22.04 container (glibc 2.35; Docker, under WSL on
Windows): `scripts/build-linux-docker.sh <output folder>` builds the committed HEAD with
`scripts/build-release-linux.sh` and checks that the engine starts. Only the .dmg is built on GitHub, by
the `macOS build` workflow (`.github/workflows/release-unix.yml`, started by hand, optionally with a
release tag to attach it to): there is no Mac on the release machine.

On macOS there is no packaged engine, so build it once from source (needs Xcode command line
tools and CMake) and again whenever `engines/yue2-cpp-source.json` moves to a new commit:

```bash
scripts/build-yue-runtime.sh ~/yue2-engine          # builds the pinned commit with Metal
YUE_ENGINE_ROOT=~/yue2-engine cargo run -p music-server
```

Audio to MIDI is Windows-only as a download; on macOS `scripts/build-midi-runtime.sh <dir>` builds HOT-Step's `ace-midi` with Metal (point `YUE_MIDI_BIN` at the resulting `music-midi`; the dmg bundles it).

`YUE_ENGINE_ROOT` (or `YUE_ENGINE_BIN`, the path of `yue-server` itself) tells the studio where
the engine is; for the desktop app set it in the environment before launching it. `Auto` then
lets the engine choose Metal and falls back to the CPU.

Next: [get started](getting-started.md) · [models and runtimes](models-and-runtimes.md) ·
[report a problem](community.md#report-a-problem).
