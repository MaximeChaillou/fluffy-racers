#!/usr/bin/env bash
# Setup script for the Claude Code cloud environment.
# Paste it into: claude.ai/code > environment settings > Setup script.
# Must finish in under ~5 minutes so the environment gets cached.
set -euo pipefail

# Bevy 0.20 needs Rust >= 1.97.1; the image may ship an older stable.
rustup toolchain install stable --profile minimal \
  --component rustfmt --component clippy --target wasm32-unknown-unknown
rustup default stable

# Native build deps for Bevy on Linux (audio, gamepads, windowing).
(apt-get update -qq && apt-get install -y -qq --no-install-recommends \
  pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev >/dev/null) &

# trunk (web build tool), prebuilt binary from GitHub releases.
(curl -sSL https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-gnu.tar.gz \
  | tar -xz -C /usr/local/bin) &

wait
rustc --version
trunk --version
