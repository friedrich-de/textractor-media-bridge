# Architecture

```text
Textractor DLL -> Windows named pipe -> Rust server -> HTTP/SSE -> Browser UI -> AnkiConnect
```

## Crates

`bridge_protocol` contains the versioned serde structs and the pipe frame format:

```text
4-byte little-endian u32 length
UTF-8 JSON payload
```

`textractor_bridge_dll` exports `OnNewSentence`, parses Textractor `InfoForExtension`, filters to the selected real text thread, and enqueues compact line events to a background named-pipe worker. The callback returns the original UTF-16 sentence pointer and wraps runtime work in `catch_unwind`.

`textractor_bridge_server` owns the durable state and browser API. It runs:

- Tokio named pipe listener
- Axum HTTP server
- SSE live event stream
- Append-only JSONL line history
- Asset storage and cleanup
- Process-to-window resolution
- Screenshot/audio backend boundaries

`web_ui` is a Vite/Vue/TypeScript app modeled after Jellyfin Miner. It loads history through `GET /api/lines`, receives live updates through `EventSource`, presents one Textractor transcript as the source of truth, supports contiguous line selection, and calls AnkiConnect from the browser. The Rust server serves `web_ui/dist` when the UI has been built.

## HTTP API

```text
GET  /api/health
GET  /api/config
GET  /api/events
GET  /api/lines?limit=&beforeSeq=&afterSeq=&sourceKey=
POST /api/lines/{line_id}/audio/finish
GET  /api/lines/{line_id}/audio/trim
POST /api/lines/{line_id}/audio/trim
POST /api/mine/prepare
POST /api/assets/{asset_id}/base64
GET  /assets/{asset_id}
```

SSE events use `id: <lineSeq>` so native `EventSource` reconnect can resume with `Last-Event-ID`. The UI also performs an `afterSeq` history fetch fallback.

## Persistence

History is an append-only JSONL log at:

```text
<data_dir>/history.jsonl
```

Assets are ID-addressed files at:

```text
<data_dir>/assets/<asset_id>
```

Periodic cleanup removes expired or storage-cap-excess assets. Lines referencing removed assets are purged with them.

## Media Backends

The server exposes screenshot and audio backend boundaries matching the specification. The current implementation prioritizes a working MVP path:

- Process window resolution uses `EnumWindows`, `GetWindowThreadProcessId`, `IsWindowVisible`, DWM cloaking checks, root-owner preference, foreground preference, and largest-window fallback.
- Screenshot capture uses Windows Graphics Capture for target windows in `auto` mode, validates the first frame, falls back to Win32 GDI `BitBlt` when WGC is unavailable or suspicious, and stores PNG assets.
- Audio sessions are tracked per line and finalize through manual finish, trailing silence, no-speech timeout, or max duration. WASAPI process loopback captures the target process tree where available, and `auto` falls back to system loopback if process activation fails.
- VAD trigger logic is implemented and tested independently. The active capture path uses PCM activity thresholds for line finalization and silence trimming. Newly captured lines store both a ready WAV trimmed from line-start audio and a broader trim-source WAV beginning one second before the line; trim edits slice the source WAV server-side and replace the ready WAV while preserving the end reason.

## LAN Mode

By default the server binds to `127.0.0.1`. If `lan_mode = true` and `session_token_required = true`, the server generates a token and logs it at startup. Protected API and asset requests accept either:

```text
?token=<token>
x-session-token: <token>
Authorization: Bearer <token>
```
