# Textractor Media Bridge

Windows-focused Rust workspace for capturing selected Textractor text-thread lines, forwarding them over a local named pipe, displaying them in a browser UI, and preparing AnkiConnect mining payloads with text plus available media.

## Workspace

```text
crates/
  bridge_protocol/             Shared JSON protocol structs and pipe framing
  textractor_bridge_dll/        Textractor cdylib extension
  textractor_bridge_server/     Tokio/Axum local server
web_ui/                         Static browser UI served by the server
docs/ARCHITECTURE.md            Architecture notes and current limitations
```

## Build

Install Rust with the MSVC toolchain. Build the browser UI first; the server embeds `web_ui/dist` into the executable:

```powershell
cd web_ui
npm install
npm run build
cd ..
```

Build the server and the DLL for your host architecture:

```powershell
cargo build --release
```

Build a 64-bit Textractor DLL:

```powershell
rustup target add x86_64-pc-windows-msvc
cargo build -p textractor_bridge_dll --release --target x86_64-pc-windows-msvc
```

Build a 32-bit Textractor DLL:

```powershell
rustup target add i686-pc-windows-msvc
cargo build -p textractor_bridge_dll --release --target i686-pc-windows-msvc
```

Artifacts:

```text
target/release/textractor_bridge_server.exe
target/<target-triple>/release/textractor_bridge_dll.dll
```

## Releases

Tagged GitHub releases build both Textractor architectures:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds the Vue UI, embeds it into the Rust server, builds x64 and x86 binaries, bundles `ffmpeg.exe`, and publishes release zips named:

```text
textractor-media-bridge-<tag>-x64.zip
textractor-media-bridge-<tag>-x86.zip
```

## Install to Textractor

The release server serves the browser app from resources embedded in `textractor_bridge_server.exe`. Install by copying the release zip contents into the Textractor folder for the matching architecture.

The folder only needs these files:

```text
textractor_bridge_server.exe
Textractor Media Bridge.xdll
ffmpeg.exe
```

## Run

Debug runs keep normal console behavior:

```powershell
cargo run -p textractor_bridge_server -- --open
```

Windows release builds start as a notification-area tray app by default. The tray menu can open the local UI, copy the local LAN URL, or quit the server. Use `--no-tray` to run the server without creating a tray icon.

Default UI on the PC:

```text
http://127.0.0.1:7788/
```

The server binds to `0.0.0.0:7788` by default so the UI is reachable from the local network. For phone/tablet access, use the tray menu's `Copy Local LAN URL` action or open:

```text
http://<PC-LAN-IP>:7788/
```

The browser app includes a web app manifest and service worker. Full PWA installation from another device normally requires HTTPS; plain HTTP LAN access is useful for testing and browser use, but mobile install prompts may be unavailable unless the page is served through a trusted HTTPS origin.

For frontend development, set `TEXTRACTOR_MEDIA_BRIDGE_WEB_UI` to a built UI directory to serve files from disk instead of the embedded bundle:

```powershell
$env:TEXTRACTOR_MEDIA_BRIDGE_WEB_UI = "C:\path\to\web_ui\dist"
```

The DLL computes a per-user pipe name using the current Windows SID. If the server is not running, the DLL worker tries to start `textractor_bridge_server.exe`; set this environment variable if the executable is not next to Textractor or on `PATH`:

```powershell
$env:TEXTRACTOR_MEDIA_BRIDGE_SERVER_EXE = "C:\path\to\textractor_bridge_server.exe"
```

## Textractor Setup

1. Build the DLL matching the Textractor/game architecture, either x86 or x64.
2. Add `textractor_bridge_dll.dll` as a Textractor extension.
3. Start `textractor_bridge_server.exe`.
4. Select the desired Textractor text thread.
5. Open the browser UI and watch the live timeline.

Only active, real text threads are forwarded:

```text
current select != 0
text number != 0
text number != 1
```

## Anki Mining

The browser talks directly to AnkiConnect/Yomitan on the device running the frontend, defaulting to:

```text
http://127.0.0.1:8765
```

The UI updates the most recently created note in the configured deck/model. Media assets are base64 fetched from the Rust server and attached through AnkiConnect `picture`/`audio` fields. Mining audio is prepared as MP3 with FFmpeg; set `mining.ffmpeg_path`, place `ffmpeg.exe` next to the server, or keep `ffmpeg` on `PATH`.

## Tests

```powershell
cargo test --workspace
cd web_ui
npm run format:check
npm run lint
npm run build
```

Current test coverage includes pipe framing, Textractor `SentenceInfo` parsing/filtering, audio range handling, asset storage, and history pagination.

## License

This project is licensed under GPL-3.0-only. See `LICENSE`.

Binary distributions may include FFmpeg as a separate GPLv3 executable for MP3 preparation.

## Current Limitations

This repository implements the architecture and a functional text/history/SSE/Vue UI/mining path. The advanced Windows media backends are intentionally isolated behind traits, but not all production capture paths are complete yet:

- Screenshot capture defaults to Windows Graphics Capture with Win32 GDI fallback. True DXGI desktop-duplication crop is not implemented.
- WASAPI process-loopback audio capture is implemented, with system loopback fallback when process loopback is unavailable. Line sessions finalize on manual finish, max duration, or the next line arriving. Main audio captures from two seconds before the line through its end; trim sources capture from ten seconds before the line and, on line advancement, include ten seconds after the line end.
- Mining prepares MP3 audio through FFmpeg. When multiple selected lines have ready audio, they are concatenated in transcript order and encoded as a single MP3.
- Named pipe security uses Tokio's named pipe creation path and does not yet install a custom current-user/admins/SYSTEM security descriptor.
