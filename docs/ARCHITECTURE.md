# Architecture

```text
Textractor DLL -> Windows named pipe -> Rust server -> HTTP/SSE + WebSocket -> Browser UI / external clients -> AnkiConnect
```

## Crates

`bridge_protocol` contains the versioned serde structs and the pipe frame format:

```text
4-byte little-endian u32 length
UTF-8 JSON payload
```

`textractor_bridge_dll` exports `OnNewSentence`, parses Textractor `InfoForExtension`, filters to the selected real text thread, and writes compact line events to the named pipe with one short-lived connection per forwarded line. The callback returns the original UTF-16 sentence pointer and wraps runtime work in `catch_unwind`.

`textractor_bridge_server` owns the durable state and browser API. It runs:

- Windows release tray shell by default, with `--no-tray` for direct server mode
- Tokio named pipe listener
- Axum HTTP server
- SSE live event stream
- Plain-text WebSocket compatibility stream starting at port `6677` and counting upward until a free port is found
- Append-only JSONL line history
- Asset storage and cleanup
- Process-to-window resolution
- Screenshot/audio media modules

`web_ui` is a Vite/Vue/TypeScript app modeled after Jellyfin Miner. It loads history through `GET /api/lines`, receives live updates through `EventSource`, presents one Textractor transcript as the source of truth, supports contiguous line selection, and calls AnkiConnect from the browser. Release builds embed `web_ui/dist` into the Rust server; frontend development can still override this with `TEXTRACTOR_MEDIA_BRIDGE_WEB_UI`.

## HTTP API

```text
GET  /api/health
GET  /api/config
POST /api/config
GET  /api/events
GET  /api/lines?limit=&beforeSeq=&afterSeq=
DELETE /api/lines
DELETE /api/lines/{line_id}/audio
POST /api/lines/{line_id}/audio/finish
GET  /api/lines/{line_id}/audio/trim
POST /api/lines/{line_id}/audio/trim
POST /api/lines/{line_id}/audio/trim/finish
POST /api/mine/prepare
POST /api/assets/{asset_id}/base64
GET  /assets/{asset_id}
```

SSE events use `id: <lineSeq>` so native `EventSource` reconnect can resume with `Last-Event-ID`. The UI also performs an `afterSeq` history fetch fallback.

## WebSocket Compatibility

The server also exposes a live WebSocket stream compatible with `textractor_websocket`:

```text
ws://localhost:6677/
```

If port `6677` is already in use, the server automatically tries `6678`, then keeps counting upward until it finds a free port:

```text
ws://localhost:6678/
```

Messages are plain text Textractor sentences, not JSON. The stream is live-only and does not replay history when a client connects. The WebSocket server runs inside `textractor_bridge_server`, not the Textractor extension DLL, so Textractor never owns a long-lived WebSocket connection or worker thread. The tray tooltip shows the actual selected port.

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

Mining preparation keeps captured line media immutable and creates derived Anki-ready assets on demand. Screenshots are transcoded to JPEG, and ready WAV clips are transcoded or concatenated to one MP3 with FFmpeg before the browser uploads them through AnkiConnect.

## Media Backends

The server keeps screenshot and audio capture isolated in focused media modules. The current implementation prioritizes a working path:

- Process window resolution uses `EnumWindows`, `GetWindowThreadProcessId`, `IsWindowVisible`, DWM cloaking checks, root-owner preference, foreground preference, and largest-window fallback.
- Screenshot capture uses Windows Graphics Capture for target windows in `auto` mode, validates the first frame, falls back to Win32 GDI `BitBlt` when WGC is unavailable or suspicious, and stores PNG assets.
- Audio sessions are tracked per line and finalize through manual finish, next-line advancement, or max duration. WASAPI process loopback captures the target process tree where available, and `auto` falls back to system loopback if process activation fails.
- Main audio captures from two seconds before the line event through the chosen line end. Trim sources capture from ten seconds before the line event and, on automatic line advancement, include ten seconds after the main line end. Trim edits slice the source WAV server-side and replace the ready WAV while preserving the current end reason.

## LAN Mode

By default the HTTP server binds to `0.0.0.0:7788`, so the browser UI and API are reachable from the local network at `http://<PC-LAN-IP>:7788/`. The WebSocket compatibility server starts at `0.0.0.0:6677` by default and increments the port until it can bind. Local browser launches use `127.0.0.1` for HTTP even when the bind address is `0.0.0.0`. The Windows tray menu can copy a concrete LAN URL by asking Windows which local IPv4 address would route to the network.
