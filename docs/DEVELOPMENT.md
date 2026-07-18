# Development

## Dev Container

Open the repository in VS Code and run **Dev Containers: Reopen in Container**. The container
installs Rust, Node.js 22, the Vue/TypeScript toolchain, Codex, and the Clang/LLD tooling needed to
cross-compile the Windows MSVC targets. It also runs `npm ci` and builds the web UI on first create.

The Codex login cache is mounted from the host at `~/.codex/auth.json`. Keep that file private; it
contains access credentials.

Build the Windows release artifacts with the VS Code build tasks, or from the container terminal:

```bash
cargo xwin build --release --target x86_64-pc-windows-msvc -p textractor_bridge_server -p textractor_bridge_dll
cargo xwin build --release --target i686-pc-windows-msvc -p textractor_bridge_server -p textractor_bridge_dll
```

## Local Server

Debug runs keep normal console behavior:

```powershell
cargo run -p textractor_bridge_server -- --open
```

Windows release builds use tray mode by default. Use `--no-tray` to run the server without creating a tray icon:

```powershell
target\release\textractor_bridge_server.exe --no-tray --open
```

## Frontend Development

Run the Vue development server:

```powershell
cd web_ui
npm install
npm run dev
```

For server-side testing with a built frontend directory, set `TEXTRACTOR_MEDIA_BRIDGE_WEB_UI` to serve files from disk instead of the embedded bundle:

```powershell
cd web_ui
npm run build
cd ..
$env:TEXTRACTOR_MEDIA_BRIDGE_WEB_UI = "C:\path\to\web_ui\dist"
cargo run -p textractor_bridge_server -- --open
```

## Server Discovery From Textractor

The DLL computes a per-user pipe name using the current Windows SID. If the server is not running, the DLL worker tries to start `textractor_bridge_server.exe` next to Textractor or from `PATH`.

Set this environment variable if the executable is elsewhere:

```powershell
$env:TEXTRACTOR_MEDIA_BRIDGE_SERVER_EXE = "C:\path\to\textractor_bridge_server.exe"
```

## Checks

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cd web_ui
npm run format:check
npm run lint
npm run build
```

Current test coverage includes pipe framing, Textractor `SentenceInfo` parsing/filtering, audio range handling, asset storage, mining preparation, and history pagination.
