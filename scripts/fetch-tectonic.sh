#!/usr/bin/env bash
# Put the Tectonic that ships beside prisme in src-tauri/binaries/, named as Tauri wants a
# sidecar (prisme-tectonic-<target triple>), its checksum checked. It is Tectonic's release built
# with scripts/tectonic-confine-outputs.patch by the `tectonic` workflow, which publishes it as the
# pre-release $RELEASE of prisme's repository.
# Then compile the documents in scripts/tectonic-seed/ with it, into a fresh cache
# (src-tauri/binaries/tectonic-seed/): the packages they use and Tectonic's format file, which ship
# with prisme so those documents compile offline from the first time.
# Usage: scripts/fetch-tectonic.sh [target triple]   (default: this machine's, from rustc)
# Nothing is fetched when the binary for that target, or a seed made from the same documents, is
# already there. The seed is made with the binary of the last target asked for, so that one must
# run here.
set -euo pipefail

VERSION=0.17.0
RELEASE="tectonic-$VERSION-prisme.1"
BASE="https://github.com/kritzerenkrieg/prisme/releases/download/$RELEASE"
DIR="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"

# The release archive for a target, and its SHA-256. Linux gets the static (musl) build either way.
asset() {
  case "$1" in
    x86_64-unknown-linux-*) echo "x86_64-unknown-linux-musl.tar.gz f9043471d1cc1fbef9a72c58e6652202bdec4719f2df2de2cadbb16cd3e1dc90" ;;
    aarch64-unknown-linux-*) echo "aarch64-unknown-linux-musl.tar.gz 66d01277ab9e2fe8f4469911d5e101430012e38cbbecae8f0af6c6bc3101a8ea" ;;
    x86_64-apple-darwin) echo "x86_64-apple-darwin.tar.gz 561b2fd770501dab18cc157bb95adc20e725dc05c1c251ff374c727bc9f27a69" ;;
    aarch64-apple-darwin) echo "aarch64-apple-darwin.tar.gz 66e5134678ad64a831155a90df71d21ef410471f65dbcccad188f463522f51a1" ;;
    x86_64-pc-windows-msvc) echo "x86_64-pc-windows-msvc.zip 80b83d8dee8bb307fb816d98a8f2b3943a3ad43b4663dd31391beb380ba780a3" ;;
    *) echo "No $RELEASE build for $1." >&2; exit 1 ;;
  esac
}

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

# Download, check and unpack the target's Tectonic to $2.
fetch() {
  read -r name sum < <(asset "$1")
  local tmp; tmp="$(mktemp -d)"
  curl -fsSL --retry 3 -o "$tmp/$name" "$BASE/$RELEASE-$name"
  local got; got="$(sha256 "$tmp/$name")"
  if [ "$got" != "$sum" ]; then
    echo "$RELEASE-$name: checksum $got, expected $sum." >&2
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
if [ ! -f "$OUT" ] || [ "$(cat "$STAMP" 2>/dev/null)" != "$RELEASE" ]; then
  mkdir -p "$DIR"
  echo "Fetching $RELEASE for $TARGET"
  if [ "$TARGET" = universal-apple-darwin ]; then
    fetch x86_64-apple-darwin "$OUT.x86_64"
    fetch aarch64-apple-darwin "$OUT.aarch64"
    lipo -create -output "$OUT" "$OUT.x86_64" "$OUT.aarch64"
    rm -f "$OUT.x86_64" "$OUT.aarch64"
  else
    fetch "$TARGET" "$OUT"
  fi
  echo "$RELEASE" > "$STAMP"
fi

# The starting cache, compiled again when the seed documents change (into what is there, so only
# new packages download), and from nothing for another Tectonic version.
SRC="$(cd "$(dirname "$0")" && pwd)/tectonic-seed"
SEED="$DIR/tectonic-seed"
WANT="$VERSION $(cat "$SRC"/*.tex "$SRC"/*.sty "$SRC"/*.bib | sha256 /dev/stdin)"
HAVE="$(cat "$SEED.version" 2>/dev/null || true)"
# The stamp inside it tells an installed prisme whether its copy of the seed is whole and current.
if [ -d "$SEED/formats" ] && [ "$HAVE" = "$WANT" ]; then
  echo "$WANT" > "$SEED/prisme-seed"
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
echo "$WANT" > "$SEED/prisme-seed"
echo "$WANT" > "$SEED.version"
du -sh "$SEED"
