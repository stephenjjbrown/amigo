#!/usr/bin/env bash
# Build the .app and launch it.
#
# Use this when you care what the app looks like: macOS only takes an app icon
# from a bundle, and winit ignores window icons on macOS, so a bare binary always
# shows the generic executable icon. `cargo run -p amigo` is faster for logic and
# uses the same preferences and logs, but it cannot show the icon.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

./scripts/bundle-macos.sh
open "$ROOT/dist/Amigo.app"
