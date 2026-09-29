#!/bin/bash
# SessionStart hook for Claude Code on the web: installs Rust, the Tauri
# Linux build dependencies and the frontend packages so a fresh cloud session
# can run the same checks as CI (.github/workflows/ci.yml) right away.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel)}"

SUDO=""
if [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
fi

# System libraries the Tauri crate links against (same list as CI).
APT_PACKAGES=(
  libwebkit2gtk-4.1-dev
  libayatana-appindicator3-dev
  librsvg2-dev
  libxdo-dev
  libssl-dev
  pkg-config
  build-essential
)
missing=()
for pkg in "${APT_PACKAGES[@]}"; do
  dpkg -s "$pkg" >/dev/null 2>&1 || missing+=("$pkg")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "Installing system packages: ${missing[*]}" >&2
  # Some preinstalled PPAs may be unreachable; the Ubuntu archive is enough.
  $SUDO apt-get update -qq >&2 || true
  DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y -qq --no-install-recommends "${missing[@]}" >&2
fi

# Rust stable with clippy and rustfmt.
if ! command -v rustup >/dev/null 2>&1; then
  echo "Installing Rust via rustup" >&2
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable >&2
  echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> "${CLAUDE_ENV_FILE:-/dev/null}"
fi
export PATH="$HOME/.cargo/bin:$PATH"
rustup component add clippy rustfmt >&2

# Frontend: the Tauri crate embeds app/build at compile time, so build it once.
(
  cd app
  npm install --no-audit --no-fund >&2
  npm run build >&2
)

# Pre-fetch and compile workspace dependencies so the first cargo run is fast.
cargo fetch >&2
cargo build --workspace --all-targets >&2
