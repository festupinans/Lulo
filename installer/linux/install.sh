#!/bin/sh
# Installs Lulo for the current user, without root:
#   ~/.local/bin/lulo-hook and lulo-widget,
#   the "Lulo" launcher and its icon,
#   Lulo's hooks in ~/.claude/settings.json (backed up first).
#
#   ./install.sh               install or update
#   ./install.sh --uninstall   remove the hooks, the programs and the launcher
#
# PREFIX changes where it goes (default ~/.local).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-"$HOME/.local"}
bin="$prefix/bin"
apps="$prefix/share/applications"
icons="$prefix/share/icons/hicolor/scalable/apps"
autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/lulo.desktop"

if [ "${1:-}" = "--uninstall" ]; then
    pkill -x lulo-widget 2>/dev/null || true
    if [ -x "$bin/lulo-hook" ]; then
        "$bin/lulo-hook" uninstall
    fi
    rm -f "$bin/lulo-hook" "$bin/lulo-widget" "$apps/lulo.desktop" "$icons/lulo.svg" "$autostart"
    echo "Lulo desinstalado. Tus ajustes siguen en ~/.local/share/lulo; bórralos si ya no los quieres."
    exit 0
fi

for f in lulo-hook lulo-widget lulo.desktop lulo.svg; do
    if [ ! -f "$here/$f" ]; then
        echo "Falta $f junto a install.sh." >&2
        exit 1
    fi
done

# An open widget keeps the old binary busy.
pkill -x lulo-widget 2>/dev/null || true

mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/lulo-hook" "$here/lulo-widget" "$bin/"
install -m 644 "$here/lulo.svg" "$icons/lulo.svg"
sed "s|^Exec=.*|Exec=$bin/lulo-widget|" "$here/lulo.desktop" > "$apps/lulo.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" 2>/dev/null || true

"$bin/lulo-hook" install --exe "$bin/lulo-hook"

# Detached, so closing this terminal leaves it open.
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    setsid -f "$bin/lulo-widget" >/dev/null 2>&1 < /dev/null || true
fi

echo
echo "Listo. Lulo quedó en $bin."
echo "Reinicia las sesiones de Claude Code abiertas para que empiecen a reportar."
case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "Aviso: $bin no está en tu PATH; el menú de aplicaciones sí encuentra a Lulo." ;;
esac
if [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
    echo "Hyprland: para abrirlo al iniciar sesión añade  exec-once = $bin/lulo-widget"
    echo "(con config Lua: hl.exec_cmd(\"$bin/lulo-widget\") dentro de hl.on(\"hyprland.start\", ...))."
fi
