# Building

Install Rust with the MSVC toolchain and Node.js/npm.

## Browser UI

Build the browser UI first. The server embeds `web_ui/dist` into the executable:

```powershell
cd web_ui
npm install
npm run build
cd ..
```

## Rust Binaries

Build the server and DLL for the host architecture:

```powershell
cargo build --release
```

Build 64-bit release binaries:

```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc -p textractor_bridge_server -p textractor_bridge_dll
```

Build 32-bit release binaries:

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p textractor_bridge_server -p textractor_bridge_dll
```

Artifacts:

```text
target/<target-triple>/release/textractor_bridge_server.exe
target/<target-triple>/release/textractor_bridge_dll.dll
```

## Release Packaging

Tagged GitHub releases build both Textractor architectures:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds the Vue UI, embeds it into the Rust server, builds x64 and x86 binaries, downloads `ffmpeg.exe`, bundles the three runtime files, and publishes release zips named:

```text
textractor-media-bridge-<tag>-x64.zip
textractor-media-bridge-<tag>-x86.zip
```

The release zip intentionally contains only files that need to be copied into the Textractor folder.
