#!/usr/bin/env bash
# Packs the release build of `target` as dist/slakio-<tag>-<target>.tar.gz (.zip on Windows):
# the binary, both licenses, the README and THIRD-PARTY-NOTICES.html (generated beforehand by
# scripts/third-party-notices.sh), in one top-level directory; next to it <archive>.sha256. When
# the runner can run the binary, its `--version` must be the tag's version.
# Usage: release-archive.sh <tag> <target>
set -euo pipefail

tag=$1
target=$2
name=slakio-$tag-$target
exe=slakio
if [[ $target == *windows* ]]; then
  exe=slakio.exe
fi

rm -rf "dist/$name"
mkdir -p "dist/$name"
cp "target/$target/release/$exe" "dist/$name/"
cp LICENSE-MIT LICENSE-APACHE README.md THIRD-PARTY-NOTICES.html "dist/$name/"

host=$(rustc -vV | sed -n 's/^host: //p')
if [[ $host == "$target" ]]; then
  said=$("dist/$name/$exe" --version | tr -d '\r')
  if [[ $said != "slakio ${tag#v}" ]]; then
    echo "the binary says \"$said\", not \"slakio ${tag#v}\"" >&2
    exit 1
  fi
  echo "$said"
fi

cd dist
if [[ $target == *windows* ]]; then
  archive=$name.zip
  7z a -tzip "$archive" "$name" > /dev/null
else
  archive=$name.tar.gz
  # No AppleDouble (._*) files from macOS tar.
  COPYFILE_DISABLE=1 tar -czf "$archive" "$name"
fi
if command -v sha256sum > /dev/null; then
  sha256sum "$archive" > "$archive.sha256"
else
  shasum -a 256 "$archive" > "$archive.sha256"
fi
rm -rf "$name"
cat "$archive.sha256"
