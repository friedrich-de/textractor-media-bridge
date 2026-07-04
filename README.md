# Textractor Media Bridge

Windows-focused Rust workspace for capturing selected Textractor text-thread lines, forwarding them over a local named pipe, displaying them in a browser UI, and preparing AnkiConnect mining payloads with text plus available media.

## Workspace

```text
crates/
  bridge_protocol/             Shared JSON protocol structs and pipe framing
  textractor_bridge_dll/        Textractor cdylib extension
  textractor_bridge_server/     Tokio/Axum local server
web_ui/                         Static browser UI served by the server
docs/                           Project documentation
```

## Documentation

- [Installation](docs/INSTALLATION.md): release zip contents, Textractor setup, tray/server runtime behavior, LAN/PWA notes, and Anki mining setup.
- [Development](docs/DEVELOPMENT.md): local server runs, frontend development, environment overrides, and checks.
- [Building](docs/BUILDING.md): UI/Rust build commands, x86/x64 targets, and release packaging.
- [Architecture](docs/ARCHITECTURE.md): crate layout, HTTP API, persistence, media backends, and LAN behavior.

## License

This project is licensed under GPL-3.0-only. See [LICENSE](LICENSE).

Binary distributions may include FFmpeg as a separate GPLv3 executable for MP3 preparation.

## Current Limitations

- Screenshot capture defaults to Windows Graphics Capture with Win32 GDI fallback. True DXGI desktop-duplication crop is not implemented.
- WASAPI process-loopback audio capture is implemented, with system loopback fallback when process loopback is unavailable.
- Named pipe security uses Tokio's named pipe creation path and does not yet install a custom current-user/admins/SYSTEM security descriptor.
