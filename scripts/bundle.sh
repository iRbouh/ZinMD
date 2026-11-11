#!/bin/sh
# Build ZinMD.app, the form macOS expects an application in. The menu bar and
# Dock take the name "ZinMD" from the bundle; a bare binary shows its file name.
#
#   scripts/bundle.sh                            quick build, for this Mac
#   scripts/bundle.sh dist                       fully optimized build, for shipping
#   scripts/bundle.sh dist x86_64-apple-darwin   the same, for another kind of Mac
set -eu
cd "$(dirname "$0")/.."

profile="${1:-release}"
target="${2:-}"
if [ -n "$target" ]; then
    cargo build --locked --profile "$profile" --target "$target"
    built="target/$target/$profile/zinmd"
else
    cargo build --profile "$profile"
    built="target/$profile/zinmd"
fi
# The version the app says it is: the one being released when a release is
# being made, otherwise the one in Cargo.toml. The system wants plain numbers
# in the bundle, 0.1.0 and not 0.1.0-rc.1.
version="${ZINMD_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
version="${version%%-*}"

app="target/ZinMD.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$built" "$app/Contents/MacOS/ZinMD"
cp assets/ZinMD.icns "$app/Contents/Resources/ZinMD.icns"
# The icon Markdown files wear in Finder when ZinMD is the app that opens them.
cp assets/ZinMD-document.icns "$app/Contents/Resources/ZinMD-document.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>ZinMD</string>
    <key>CFBundleDisplayName</key><string>ZinMD</string>
    <key>CFBundleIdentifier</key><string>app.zinmd.ZinMD</string>
    <key>CFBundleExecutable</key><string>ZinMD</string>
    <key>CFBundleIconFile</key><string>ZinMD</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Markdown document</string>
            <key>CFBundleTypeRole</key><string>Editor</string>
            <key>CFBundleTypeIconFile</key><string>ZinMD-document</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array><string>net.daringfireball.markdown</string><string>public.plain-text</string></array>
            <key>CFBundleTypeExtensions</key>
            <array><string>md</string><string>markdown</string><string>mdx</string><string>mdown</string><string>mkd</string></array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key><string>Folder</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>None</string>
            <key>LSItemContentTypes</key>
            <array><string>public.folder</string></array>
        </dict>
    </array>
</dict>
</plist>
PLIST

# Signed "ad hoc", with no certificate. A Mac with Apple Silicon runs nothing
# unsigned, and a seal over the whole bundle keeps the system from calling a
# downloaded copy damaged. A real signature and notarization come later.
codesign --force --deep --sign - "$app" 2>/dev/null

echo "built $app ($(du -sh "$app" | cut -f1))"
