#!/bin/sh
set -e

dir="$(realpath "$(dirname "$0")")"
version="$(grep '^version' "$dir/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
tag="v${version}"
release_exe="$dir/target/release/mcp-reauth"
release_exe_renamed="/tmp/$(basename "$release_exe")-macos"
host="github.com"
repo="${host}/$(git -C "$dir" remote get-url origin | sed 's/.*:\(.*\)\.git/\1/')"

echo "Building release ${tag}..."
cargo build --release --manifest-path "$dir/Cargo.toml"
cp "$release_exe" "$release_exe_renamed"

echo "Creating GitHub release ${tag}..."
gh release create "$tag" "$release_exe_renamed" \
    --repo "$repo" \
    --title "$tag" \
    --generate-notes \
    --notes "$(cat <<'NOTES'
## Artifacts

* `mcp-reauth-macos`: The unsigned release build of the application for MacOS.
NOTES
)"

rm -f "$release_exe_renamed"

