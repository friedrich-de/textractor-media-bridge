FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

USER root

# cargo-xwin uses Clang/LLD to link against the downloaded MSVC CRT and
# Windows SDK. Ninja/CMake cover native dependencies that use cmake-rs.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        clang \
        cmake \
        lld \
        llvm \
        ninja-build \
        pkg-config \
    && ln -s /usr/bin/clang /usr/local/bin/clang-cl \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy llvm-tools rustfmt \
    && rustup target add \
        i686-pc-windows-msvc \
        x86_64-pc-windows-msvc \
    && cargo install --locked cargo-xwin \
    && rm -rf /usr/local/cargo/registry /usr/local/cargo/git

# Install the standalone Codex CLI in a shared location. Runtime credentials
# remain under /home/vscode/.codex and are supplied by devcontainer.json.
RUN curl -fsSL https://chatgpt.com/codex/install.sh -o /tmp/install-codex.sh \
    && CODEX_HOME=/opt/codex \
        CODEX_INSTALL_DIR=/usr/local/bin \
        CODEX_NON_INTERACTIVE=1 \
        sh /tmp/install-codex.sh \
    && rm /tmp/install-codex.sh \
    && mkdir -p /home/vscode/.codex /home/vscode/.cache/cargo-xwin \
    && chown -R vscode:vscode /home/vscode/.codex /home/vscode/.cache

USER vscode
