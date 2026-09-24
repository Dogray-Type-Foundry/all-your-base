#!/bin/sh
# Builds the autobase wrapper for arm64 and x86_64 and puts the universal
# dylib into the plugin bundle. The first build downloads the word lists.
set -e
cd "$(dirname "$0")/rust"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
	cargo build --release --target "$target"
done
# Replace rather than overwrite: macOS rejects an in-place rewrite of a dylib it has loaded before
rm -f "../All Your BASE.glyphsPlugin/Contents/Resources/libautobase_glyphs.dylib"
lipo -create \
	target/aarch64-apple-darwin/release/libautobase_glyphs.dylib \
	target/x86_64-apple-darwin/release/libautobase_glyphs.dylib \
	-output "../All Your BASE.glyphsPlugin/Contents/Resources/libautobase_glyphs.dylib"
install_name_tool -id @rpath/libautobase_glyphs.dylib "../All Your BASE.glyphsPlugin/Contents/Resources/libautobase_glyphs.dylib"
codesign --force --sign - "../All Your BASE.glyphsPlugin/Contents/Resources/libautobase_glyphs.dylib"
