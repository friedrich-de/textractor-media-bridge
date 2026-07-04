# Textractor Media Bridge

Windows-focused Rust workspace for capturing selected Textractor text-thread lines, forwarding them over a local named pipe, displaying them in a browser UI, and preparing AnkiConnect mining payloads with text plus available media.


## Documentation

- [Installation](docs/INSTALLATION.md): release zip contents, Textractor setup, tray/server runtime behavior, LAN/PWA notes, and Anki mining setup.
- [Development](docs/DEVELOPMENT.md): local server runs, frontend development, environment overrides, and checks.
- [Building](docs/BUILDING.md): UI/Rust build commands, x86/x64 targets, and release packaging.
- [Architecture](docs/ARCHITECTURE.md): crate layout, HTTP API, persistence, media backends, and LAN behavior.