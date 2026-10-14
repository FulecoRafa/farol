#!/bin/sh
# farol inside the Flatpak sandbox: no arguments runs `farol start` (the
# runner, which starts the core); anything else goes to the farol CLI
# (`toggle`, `doctor`, `hotkey`, `set`, ...).
#
# The programs farol starts run on the host through flatpak-spawn --host
# (the core does that itself when FLATPAK_ID is set), so PATH stays the
# sandbox's.
set -eu

# Desktop entries and icons: the app's own, the user's and the host's.
# (host-os exposes the host's /usr under /run/host/usr.)
export XDG_DATA_DIRS="${XDG_DATA_DIRS:-/app/share:/usr/share}:$HOME/.local/share:$HOME/.local/share/flatpak/exports/share:/var/lib/flatpak/exports/share:/run/host/usr/local/share:/run/host/usr/share"

# Wayland: Flatpak unsets WAYLAND_DISPLAY when the app has no --socket=wayland
# (see the manifest for why it has none). FAROL_WAYLAND_DISPLAY names the
# socket; otherwise the first of wayland-0, wayland-1 that was shared.
if [ -n "${FAROL_WAYLAND_DISPLAY:-}" ]; then
    export WAYLAND_DISPLAY="$FAROL_WAYLAND_DISPLAY"
else
    for d in wayland-0 wayland-1; do
        if [ -S "$XDG_RUNTIME_DIR/$d" ]; then
            export WAYLAND_DISPLAY="$d"
            break
        fi
    done
fi

if [ $# -eq 0 ]; then
    if [ ! -S "$XDG_RUNTIME_DIR/${WAYLAND_DISPLAY:-none}" ]; then
        echo "farol: no Wayland socket shared with the sandbox (see README.md)" >&2
        exit 1
    fi
    set -- start
fi
exec /app/bin/farol "$@"
