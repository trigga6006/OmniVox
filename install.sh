#!/usr/bin/env bash
#
# OmniVox Installer — downloads and installs the latest release for macOS/Linux.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/trigga6006/OmniVox/main/install.sh | bash
#

set -euo pipefail

REPO="trigga6006/OmniVox"
APP_NAME="OmniVox"

echo ""
echo "  ╔══════════════════════════════════════╗"
echo "  ║         OmniVox Installer            ║"
echo "  ║       Local AI Dictation             ║"
echo "  ╚══════════════════════════════════════╝"
echo ""

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        PLATFORM="macos"
        if [ "$ARCH" = "arm64" ]; then
            ASSET_PATTERN="aarch64.dmg"
        else
            ASSET_PATTERN="x64.dmg"
        fi
        ;;
    Linux)
        PLATFORM="linux"
        if [ "$ARCH" = "x86_64" ]; then
            # Prefer AppImage for broad compatibility
            ASSET_PATTERN="amd64.AppImage"
        elif [ "$ARCH" = "aarch64" ]; then
            ASSET_PATTERN="arm64.AppImage"
        else
            echo "  ERROR: Unsupported architecture: $ARCH" >&2
            exit 1
        fi
        ;;
    *)
        echo "  ERROR: Unsupported OS: $OS" >&2
        echo "  Use install.ps1 for Windows." >&2
        exit 1
        ;;
esac

echo "  Detected: $PLATFORM ($ARCH)"
echo "  Fetching latest release..."

# Fetch latest release info from GitHub
RELEASE_JSON=$(curl -fsSL \
    -H "User-Agent: OmniVox-Installer" \
    "https://api.github.com/repos/$REPO/releases/latest") || {
    echo "  ERROR: Could not reach GitHub. Check your internet connection." >&2
    exit 1
}

TAG=$(echo "$RELEASE_JSON" | grep -o '"tag_name":\s*"[^"]*"' | head -1 | cut -d'"' -f4)
echo "  Latest version: $TAG"

# Find the matching asset URL
DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o '"browser_download_url":\s*"[^"]*"' | cut -d'"' -f4 | grep "$ASSET_PATTERN" | head -1)

if [ -z "$DOWNLOAD_URL" ]; then
    echo "  ERROR: No $PLATFORM release asset found matching '$ASSET_PATTERN'." >&2
    echo "  Check https://github.com/$REPO/releases for available downloads." >&2
    exit 1
fi

FILENAME=$(basename "$DOWNLOAD_URL")
TMPDIR=$(mktemp -d)
TMPFILE="$TMPDIR/$FILENAME"

echo "  Downloading $FILENAME..."
curl -fSL --progress-bar -o "$TMPFILE" "$DOWNLOAD_URL" || {
    echo "  ERROR: Download failed." >&2
    rm -rf "$TMPDIR"
    exit 1
}

echo "  Download complete."

if [ "$PLATFORM" = "macos" ]; then
    echo "  Mounting DMG..."
    MOUNT_POINT=$(hdiutil attach "$TMPFILE" -nobrowse -noautoopen 2>/dev/null | grep "/Volumes" | awk '{print $NF}')

    if [ -z "$MOUNT_POINT" ]; then
        # Try extracting the full mount path
        MOUNT_POINT=$(hdiutil attach "$TMPFILE" -nobrowse -noautoopen 2>/dev/null | tail -1 | awk -F'\t' '{print $NF}')
    fi

    APP_PATH="$MOUNT_POINT/$APP_NAME.app"

    if [ ! -d "$APP_PATH" ]; then
        echo "  ERROR: Could not find $APP_NAME.app in DMG." >&2
        hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true
        rm -rf "$TMPDIR"
        exit 1
    fi

    echo "  Installing to /Applications..."
    # Remove old version if present
    if [ -d "/Applications/$APP_NAME.app" ]; then
        rm -rf "/Applications/$APP_NAME.app"
    fi
    cp -R "$APP_PATH" "/Applications/"

    echo "  Cleaning up..."
    hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true

    echo ""
    echo "  ✓ $APP_NAME installed to /Applications/$APP_NAME.app"
    echo ""
    echo "  NOTE: On first launch macOS will ask for:"
    echo "    • Microphone access (for voice recording)"
    echo "    • Accessibility access (for global hotkey & text pasting)"
    echo "  Grant both in System Settings → Privacy & Security."
    echo ""
    echo "  To launch: open /Applications/$APP_NAME.app"

elif [ "$PLATFORM" = "linux" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"

    INSTALL_PATH="$INSTALL_DIR/$APP_NAME.AppImage"
    mv "$TMPFILE" "$INSTALL_PATH"
    chmod +x "$INSTALL_PATH"

    echo ""
    echo "  ✓ $APP_NAME installed to $INSTALL_PATH"
    echo ""
    echo "  To launch: $INSTALL_PATH"
    echo ""

    # Check if ~/.local/bin is in PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -q "$INSTALL_DIR"; then
        echo "  TIP: Add ~/.local/bin to your PATH:"
        echo '    export PATH="$HOME/.local/bin:$PATH"'
        echo ""
    fi
fi

rm -rf "$TMPDIR"
echo "  Done!"
