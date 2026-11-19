#!/bin/sh
# Builds Pina Live.app and installs it in /Applications (or the folder given). Quit Pina Live
# first: the app is replaced whole.
#
#   scripts/bundle.sh [folder]
set -eu
cd "$(dirname "$0")/.."
DEST="${1:-/Applications}"
if pgrep -x pina-live >/dev/null; then
  echo "Pina Live is open: quit it first." >&2
  exit 1
fi
cargo build --release -p pina-live
APP="$DEST/Pina Live.app"
scripts/mac-app.sh target/release/pina-live "$APP"
touch "$APP"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP"
echo "$APP"
