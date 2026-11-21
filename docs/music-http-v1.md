# Signed music adapter protocol

A Studio request selects a verified engine and model pair.

```json
{
  "music_source": {
    "kind": "signed",
    "engine": "example-ace",
    "model": "example-turbo"
  },
  "style": "Instrumental funk groove",
  "lyrics": "[Instrumental]",
  "duration_seconds": 4,
  "steps": 8,
  "seed": 42,
  "output_format": "wav32"
}
```

Studio selects the active generation of each package. An optional `generation`
or `model_generation` chooses an exact installed generation. Studio verifies
both packages against the signed catalogue before it starts the executable.
The signed model entry must list the engine under `engines`.

Studio launches the verified executable without arguments in its package
directory. Package dependencies must come from the verified package or the
operating system. The adapter receives the following environment variables.

| Variable | Purpose |
| --- | --- |
| `YUE_MUSIC_ADAPTER_HOST` | Bind to `127.0.0.1` |
| `YUE_MUSIC_ADAPTER_PORT` | Loopback port chosen by Studio |
| `YUE_MUSIC_ADAPTER_TOKEN` | One process's secret, required by every handler as an `Authorization: Bearer` token |
| `YUE_MUSIC_ADAPTER_ID` | ID of the verified engine |
| `YUE_MUSIC_ADAPTER_VERSION` | Version of the verified engine |
| `YUE_MUSIC_ADAPTER_MODELS` | Absolute path to the verified model generation's read-only files |

The adapter exits and reaps any children when Studio closes its stdin. Each
process serves one Studio job. Studio retires the process after it reads the
result, acknowledges cancellation, or encounters a transport failure.

| Method | Request | Response |
| --- | --- | --- |
| GET | `/v1/health` | `{"protocol":"music-http-v1","engine_id":"example-ace","version":"1.0"}` |
| POST | `/v1/jobs` with a submission | `{"id":"job-123"}` |
| GET | `/v1/jobs/job-123` | `{"status":"running","message":"Optional progress"}` |
| GET | `/v1/jobs/job-123/result` after completion | A WAV and replay object |
| POST | `/v1/jobs/job-123/cancel` | The same status response with `cancelled` once the engine accepts the stop |

Return `503` from health while getting ready. Job IDs use up to 128 ASCII
letters, digits, underscores or hyphens. A job reports `queued`, `running`,
`completed`, `failed`, or `cancelled`. A failure may include a `message`.
Other failing HTTP responses may include an `error` string.

Health may include `compute_backend` once the adapter knows what will run the
job. The supported values are `cpu`, `cuda`, `vulkan` and `metal`.
An omitted or unrecognized value reports `unknown` for speed attribution.

The submission object has a string `model`, a `mode` of `create` or `replay`,
and a `payload` object. Create can send `style`, `lyrics`, `duration_seconds`,
`seed`, `lm_seed`, `steps`, `cfg_scale`, `abc`, `solver` and `cot`. An
adapter should reject controls it cannot honor. Replay sends
`{"saved":{...},"overrides":{"seed":null,"steps":null,"solver":null}}` and
uses the exact verified engine and model generations stored with the source
song. A null override keeps the original value.

A successful result returns `audio_base64` as standard Base64 of one PCM WAV,
an object `replay` containing enough native state to re-render, and optional
`settings` with the actual parameters. Control responses are limited to 1 MiB
and the result JSON is limited to 384 MiB. Studio shapes the WAV for the
requested export format and saves the signed identities separately from
adapter-provided settings.

For a native reference build, run
`cargo build --locked -p music-server --example music_http_v1_ace`.
Package the resulting adapter alongside the native `ace-server` and its
runtime files. It uses the signed model directory for ACE-Step's weights.
