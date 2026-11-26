#!/usr/bin/env bash
# Put the Tectonic that ships beside prisme in src-tauri/binaries/, named as Tauri wants a
# sidecar (prisme-tectonic-<target triple>), from Tectonic's own release, its checksum checked.
# Then compile the documents in scripts/tectonic-seed/ with it, into a fresh cache
# (src-tauri/binaries/tectonic-seed/): the packages they use and Tectonic's format file, which ship
# with prisme so those documents compile offline from the first time.
# Usage: scripts/fetch-tectonic.sh [target triple]   (default: this machine's, from rustc)
# Nothing is fetched when the binary for that target, or a seed made from the same documents, is
# already there. The seed is made with the binary of the last target asked for, so that one must
# run here.
set -euo pipefail

VERSION=0.17.0
BASE="https://github.com/tectonic-typesetting/tectonic/releases/download/tectonic%40$VERSION"
DIR="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"

# The release archive for a target, and its SHA-256. Linux gets the static (musl) build either way.
asset() {
  case "$1" in
    x86_64-unknown-linux-*) echo "x86_64-unknown-linux-musl.tar.gz 8533d07f9ccbd7a65824b9e0459041bca34af1eb33daba48f59215593753a3b7" ;;
    aarch64-unknown-linux-*) echo "aarch64-unknown-linux-musl.tar.gz b10954a95404f3ab2328d2fa59a5ebab8e657f893fab096f98be8db7c0c979b8" ;;
    x86_64-apple-darwin) echo "x86_64-apple-darwin.tar.gz 7c90ef5b6ddb1eb1937e4337add5237b79338e4b9676459fa91187d24d6cdf80" ;;
    aarch64-apple-darwin) echo "aarch64-apple-darwin.tar.gz a3f1cac7c5678f01661a92212f58480ae3b0634115d880dbc59e2953ded45667" ;;
    x86_64-pc-windows-msvc) echo "x86_64-pc-windows-msvc.zip f61ce51f0b0ade1015b7de7ef368541c5424e9756ecbd0d7af97d6d48030845f" ;;
    *) echo "No Tectonic $VERSION release for $1." >&2; exit 1 ;;
  esac
}

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

# Download, check and unpack the target's Tectonic to $2.
fetch() {
  read -r name sum < <(asset "$1")
  local tmp; tmp="$(mktemp -d)"
  curl -fsSL --retry 3 -o "$tmp/$name" "$BASE/tectonic-$VERSION-$name"
  local got; got="$(sha256 "$tmp/$name")"
  if [ "$got" != "$sum" ]; then
    echo "tectonic-$VERSION-$name: checksum $got, expected $sum." >&2
    exit 1
  fi
  case "$name" in
    *.zip) (cd "$tmp" && if command -v unzip >/dev/null; then unzip -q "$name"; else powershell -NoProfile -Command "Expand-Archive -Path '$name' -DestinationPath ."; fi) ;;
    *) tar -xzf "$tmp/$name" -C "$tmp" ;;
  esac
  local exe=tectonic; [[ "$name" == *.zip ]] && exe=tectonic.exe
  mv "$tmp/$exe" "$2"
  chmod +x "$2"
  rm -rf "$tmp"
}

TARGET="${1:-${TAURI_ENV_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}}"
EXT=""; [[ "$TARGET" == *windows* ]] && EXT=".exe"
OUT="$DIR/prisme-tectonic-$TARGET$EXT"
STAMP="$OUT.version"
if [ ! -f "$OUT" ] || [ "$(cat "$STAMP" 2>/dev/null)" != "$VERSION" ]; then
  mkdir -p "$DIR"
  echo "Fetching Tectonic $VERSION for $TARGET"
  if [ "$TARGET" = universal-apple-darwin ]; then
    fetch x86_64-apple-darwin "$OUT.x86_64"
    fetch aarch64-apple-darwin "$OUT.aarch64"
    lipo -create -output "$OUT" "$OUT.x86_64" "$OUT.aarch64"
    rm -f "$OUT.x86_64" "$OUT.aarch64"
  else
    fetch "$TARGET" "$OUT"
  fi
  echo "$VERSION" > "$STAMP"
fi

# The starting cache, compiled again when the seed documents change (into what is there, so only
# new packages download), and from nothing for another Tectonic version.
SRC="$(cd "$(dirname "$0")" && pwd)/tectonic-seed"
SEED="$DIR/tectonic-seed"
WANT="$VERSION $(cat "$SRC"/*.tex "$SRC"/*.sty "$SRC"/*.bib | sha256 /dev/stdin)"
HAVE="$(cat "$SEED.version" 2>/dev/null || true)"
if [ -d "$SEED/formats" ] && [ "$HAVE" = "$WANT" ]; then
  exit 0
fi
[ "${HAVE%% *}" = "$VERSION" ] || rm -rf "$SEED"
rm -f "$SEED.version"
echo "Compiling the seed documents with Tectonic (downloads the TeX packages they use)"
WORK="$(mktemp -d)"
cp "$SRC"/*.tex "$SRC"/*.sty "$SRC"/*.bib "$WORK"/
for doc in "$WORK"/*.tex; do
  echo "  $(basename "$doc")"
  TECTONIC_CACHE_DIR="$SEED" "$OUT" --chatter minimal --outdir "$WORK" "$doc"
done
rm -rf "$WORK"
echo "$WANT" > "$SEED.version"
du -sh "$SEED"
