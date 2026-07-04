# Installation

## Install Textractor

Textractor is required. I recommend:

https://github.com/Chenx221/Textractor

The Releases tab may show an older release date, but newer builds are appended to the bottom of the release page. Scroll down and download the newest matching build.

## Install to Textractor

Download the release zip matching your Textractor/game architecture:

```text
textractor-media-bridge-<tag>-x64.zip
textractor-media-bridge-<tag>-x86.zip
```

Copy the zip contents (three files) into the matching Textractor folder, next to `Textractor.exe`.

```text
textractor_bridge_server.exe
Textractor Media Bridge.xdll/.dll
ffmpeg.exe
```

## Textractor Setup

1. Add the bridge extension DLL or XDLL as a Textractor extension. Ensure it shows up in the list of extensions. If not, drag and drop the DLL or XDLL.

![Textractor Extensions window showing Textractor Media Bridge](pics/textractor-extensions.png)

2. Start Textractor and attach your game.
3. Select the desired Textractor text thread.
4. Let the extension start `textractor_bridge_server.exe`, or start it manually. It starts after advancing text for the first time.

> **Network access**
>
> Windows Firewall may ask whether to allow `textractor_bridge_server.exe` on the network. Allow access on private networks if you want to open the web UI from another device, such as a phone or tablet. This makes the bridge reachable on your local LAN.

5. It should be open in the tray and let you open the web interface from there.

> **WebSocket compatibility**
>
> If you previously installed [`textractor_websocket`](https://github.com/kuroahna/textractor_websocket), remove or disable that extension before using Textractor Media Bridge. This bridge now provides a compatible WebSocket stream from the server process, so the old extension is no longer needed and can conflict with Textractor.

![Textractor Media Bridge tray menu](pics/tray.png)

Default UI on the PC:

```text
http://127.0.0.1:7788/
```

## Mining from other devices

To add media to cards from another device, add the UI origin to your allowed CORS list. For example, allow:

```text
http://192.168.178.120:7788
```

On Android, if you need multiple allowed CORS origins, use this AnkiConnect Android fork:

https://github.com/friedrich-de/AnkiconnectAndroid-Kai

## WebSocket Compatibility

The server exposes a plain-text WebSocket stream compatible with `textractor_websocket`:

```text
ws://localhost:6677/
```

If port `6677` is already occupied, the bridge automatically tries `6678`, then keeps counting upward until it finds a free port:

```text
ws://localhost:6678/
```

Messages are live Textractor sentence text only. The tray tooltip shows the actual selected WebSocket port. Do not run the old `textractor_websocket` extension at the same time unless you intentionally want a separate WebSocket stream on another port.

## Anki Mining

The browser talks directly to AnkiConnect/Yomitan on the device running the frontend, defaulting to:

```text
http://127.0.0.1:8765
```

Before mining, make sure AnkiConnect is reachable and your Anki setup is ready.

The UI updates the newest recent note matching the configured note type and target fields. Media assets are base64 fetched from the Rust server and attached through AnkiConnect `picture`/`audio` fields. Mining audio is prepared as MP3 with FFmpeg from the bundled `ffmpeg.exe`.
