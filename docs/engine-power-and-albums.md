# Music engine power / player-only mode

## Using the controls

When a complete model set is installed, the **Create music** header shows **Stop engine** beside **Engine ready**. Use it when you want to play saved songs, browse the library, DJ, or work on other GPU-heavy software without YuE2's generation engine retaining GPU allocations.

Stopping shuts down the studio-owned **yue-server** generation process (and frees its allocations); it does **not** close YuE2 Studio, unload the song library, stop the Web Audio player, or remove any model files. **Engine stopped** and **Start engine** then appear at the top of Create. Starting uses the existing engine startup path and may reload large model weights.

**Engine ready** normally means the generation process is available, not that every model is necessarily held in VRAM. YuE2 also automatically evicts kept models after its idle interval (normally 10 minutes); stopping is a more definite way to release the engine process's allocations.

The control refuses to stop when generation or training is still running, to avoid losing the work. It also refuses to terminate a music-engine process the studio did not start. If anything refuses, the error appears beside the header.

This manual choice lasts until the user clicks Start, restarts through Engine Settings, or closes the app. Starting a fresh application session uses the normal automatic engine startup behaviour. The background crash-recovery watchdog deliberately ignores a manually stopped engine during the current session.

## Implementation

- `GET /engine/power` reads whether YuE2 has manually paused generation and whether its owned engine process is alive.
- `POST /engine/power` with `{"running":false}` stops the owned engine after checking active music/training jobs. `{"running":true}` starts it with the saved GPU/CPU/model options.
- The supervisor respects `engine_manually_paused`, so it does not immediately restart the process.
- The UI polls the power status and leaves normal audio playback untouched.

## Adding songs to albums

The standard song right-click menu now offers **Add to Album** next to **Add to Playlist**. Both reuse the existing collection storage and editing flow, but each picker filters out the other collection type. The album picker can create a new album with the selected songs already included.
