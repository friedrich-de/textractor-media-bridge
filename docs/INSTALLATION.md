# Installation

Release builds are self-contained for the browser UI: the server serves the web app from resources embedded in `textractor_bridge_server.exe`.

## Install to Textractor

Download the release zip matching your Textractor/game architecture:

```text
textractor-media-bridge-<tag>-x64.zip
textractor-media-bridge-<tag>-x86.zip
```

Copy the zip contents into the matching Textractor folder. The folder only needs these files:

```text
textractor_bridge_server.exe
Textractor Media Bridge.xdll
ffmpeg.exe
```

For x64 Textractor, the extension file is named:

```text
Textractor Media Bridge.dll
```

## Textractor Setup

1. Add the bridge extension DLL or XDLL as a Textractor extension.
2. Start Textractor and attach your game.
3. Select the desired Textractor text thread.
4. Let the extension start `textractor_bridge_server.exe`, or start it manually.
5. Open the browser UI and watch the live timeline.

Only active, real text threads are forwarded:

```text
current select != 0
text number != 0
text number != 1
```

## Running

Windows release builds start as a notification-area tray app by default. The tray menu can open the local UI, copy the local LAN URL, or quit the server.

Default UI on the PC:

```text
http://127.0.0.1:7788/
```

The server binds to `0.0.0.0:7788` by default so the UI is reachable from the local network. For phone/tablet access, use the tray menu's `Copy Local LAN URL` action or open:

```text
http://<PC-LAN-IP>:7788/
```

The browser app includes a web app manifest and service worker. Full PWA installation from another device normally requires HTTPS; plain HTTP LAN access is useful for testing and browser use, but mobile install prompts may be unavailable unless the page is served through a trusted HTTPS origin.

## Anki Mining

The browser talks directly to AnkiConnect/Yomitan on the device running the frontend, defaulting to:

```text
http://127.0.0.1:8765
```

The UI updates the most recently created note in the configured deck/model. Media assets are base64 fetched from the Rust server and attached through AnkiConnect `picture`/`audio` fields. Mining audio is prepared as MP3 with FFmpeg from the bundled `ffmpeg.exe`.
