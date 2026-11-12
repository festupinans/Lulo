# Instalar Lulo

Lulo es una media luna pequeña pegada al borde de arriba de la pantalla que muestra en vivo qué hace cada sesión de Claude Code en tu PC.

Esta guía es para Windows. Si usas Linux, ve a [Linux](#linux).

## Qué necesitas

- Windows 10 u 11.
- Claude Code en este PC, en la terminal o en la pestaña Code de la app de escritorio de Claude.

Lulo ve las sesiones que corren en tu PC. Las sesiones en la nube (claude.ai en el navegador, proyectos o Claude Code en la web), por SSH, en WSL o de Cowork no aparecen.

## Instalación

1. Abre la [última versión](https://github.com/festupinans/Lulo/releases/latest) y descarga `Lulo-Setup.exe`.
2. Ábrelo. No pide permisos de administrador.
   - Windows mostrará «Windows protegió su PC» porque Lulo no está firmado. Pulsa **Más información** y luego **Ejecutar de todas formas**. Lo explica la sección [Por qué sale el aviso de Windows](#por-qué-sale-el-aviso-de-windows).
3. Deja marcado **Iniciar Lulo con Windows** si quieres que arranque solo, pulsa **Siguiente** y luego **Finalizar**. El instalador copia Lulo a `%LOCALAPPDATA%\Lulo`, añade los hooks a `~/.claude/settings.json` (guardando antes una copia del archivo), crea el acceso «Lulo» en el menú Inicio y abre el widget.
4. Reinicia las sesiones de Claude Code que tengas abiertas. En la app de escritorio basta con cerrarla y volver a abrirla. Desde ese momento cada sesión aparece como un punto en la media luna.

### Sin instalador

`lulo-windows-x64.zip` trae los dos programas sueltos. Descomprímelo, deja `lulo-hook.exe` y `lulo-widget.exe` en la misma carpeta y haz doble clic en `lulo-hook.exe`: se copia a la misma carpeta que usa el instalador y añade los hooks. Así Lulo no aparece en Configuración > Aplicaciones y se desinstala a mano (ver abajo).

## Uso

- **Plegada:** la media luna con los ojos del pulpo, que cuelga de cabeza, y un punto de color por cada sesión activa. Los ojos siguen a la sesión que más te necesita.
- **Al pasar el ratón:** aparece una ficha por sesión con su proyecto, su estado (pensando, editando, leyendo, en el bash, subagente, esperando permiso, terminó, error, inactiva…) y cuánto tiempo lleva en él.
- **Tareas en segundo plano:** si una sesión tiene comandos o monitores corriendo aparte, su ficha muestra un punto morado y cuántos son («+1», «+2»…).
- **Clic en una ficha:** trae al frente la ventana de esa sesión.
- **Avisos:** una notificación y un sonido cuando una sesión te espera, termina o falla.
- **Clic derecho:** Iniciar con Windows, Avisos de Windows, Sonidos, Esconder en pantalla completa, Pantalla, Posición (también arriba o abajo), Tamaño y Cerrar Lulo. Sobre una sesión inactiva, también Quitar de la lista.
- **Volver a abrirla:** busca «Lulo» en el menú Inicio.

Los ajustes están en `%LOCALAPPDATA%\Lulo\widget.json`. Casi todos se cambian desde el clic derecho; estos solo desde el archivo:

- `"animate": false` deja el pulpo quieto y gasta menos.
- `"inactive_minutes"` (5 por defecto): minutos sin actividad antes de marcar una sesión como «Inactiva».
- `"forget_hours"` (12 por defecto): horas sin actividad antes de que Lulo olvide la sesión.
- `"new_session_sound": true` suena una burbuja cuando abres una sesión nueva.
- `"size"` (100 por defecto): tamaño en porcentaje, entre 50 y 200. El menú **Tamaño** ofrece 80, 100 y 120.

Después de editarlo, cierra Lulo (clic derecho > **Cerrar Lulo**) y ábrelo otra vez desde el menú Inicio.

## Actualizar

Descarga el `Lulo-Setup.exe` de la nueva versión y ábrelo. Cierra el widget, lo reemplaza y nunca duplica los hooks. También sirve si antes instalaste con el zip.

## Desinstalar

En **Configuración > Aplicaciones > Aplicaciones instaladas**, busca **Lulo** y pulsa **Desinstalar**. Quita los hooks de `settings.json` (dejando el resto como estaba), el inicio con Windows y las carpetas de Lulo.

Si instalaste con el zip:

1. Clic derecho sobre la media luna, desmarca **Iniciar con Windows** y pulsa **Cerrar Lulo**.
2. En una terminal, ejecuta:

   ```
   %LOCALAPPDATA%\Lulo\lulo-hook.exe uninstall
   ```

   Quita los hooks de Lulo de `settings.json` y deja el resto como estaba.
3. Borra las carpetas `%LOCALAPPDATA%\Lulo` y `%LOCALAPPDATA%\claude-status`.

## Si algo no funciona

- **Una sesión no aparece:** reiníciala. Las sesiones que ya estaban abiertas al instalar no cargan los hooks hasta que se reinician.
- **Una sesión se queda en un estado viejo:** pasa a «Inactiva» tras 5 minutos sin eventos y el widget la olvida tras 12 horas. Puedes quitarla antes con clic derecho > **Quitar de la lista**.
- **No ves la media luna:** puede estar escondida porque hay algo en pantalla completa, o en otro monitor. Ábrela desde el menú Inicio y revisa **Pantalla** en el clic derecho.
- **No suena ni avisa:** revisa **Sonidos** y **Avisos de Windows** en el clic derecho, y que el modo «No molestar» de Windows esté apagado.
- **Quieres ver dónde escribe Lulo:** `%LOCALAPPDATA%\Lulo\lulo-hook.exe status-dir` muestra la carpeta con un archivo por sesión.

## Linux

### Qué necesitas

- Linux x86_64 con un escritorio gráfico: X11, o Wayland con XWayland (GNOME, KDE, Hyprland, Sway…).
- Claude Code en la terminal.
- Recomendado: `xrandr` (monitores), `notify-send` (avisos) y `pw-play`, `paplay` o `aplay` (sonidos). En Hyprland, `hyprctl` ya viene. En otros escritorios X11, `wmctrl` permite el clic que trae la terminal al frente.

### Instalación

1. Abre la [última versión](https://github.com/festupinans/Lulo/releases/latest) y descarga `lulo-linux-x64.tar.gz`.
2. En una terminal:

   ```sh
   tar xzf lulo-linux-x64.tar.gz
   ./lulo/install.sh
   ```

   No pide root. Copia `lulo-hook` y `lulo-widget` a `~/.local/bin`, crea el acceso «Lulo» en el menú de aplicaciones, añade los hooks a `~/.claude/settings.json` (guardando antes una copia del archivo) y abre el widget.
3. Reinicia las sesiones de Claude Code que tengas abiertas.

Para compilarlo tú: `cargo build --release` y luego copia `target/release/lulo-hook`, `target/release/lulo-widget`, `installer/linux/install.sh`, `installer/linux/lulo.desktop` y `assets/lulo.svg` a una misma carpeta y ejecuta `install.sh` desde ahí.

### Iniciar con la sesión

Clic derecho sobre la media luna > **Iniciar con la sesión** crea `~/.config/autostart/lulo.desktop`, que GNOME, KDE, XFCE y la mayoría de escritorios ejecutan al entrar.

Hyprland, Sway y otros compositores sueltos no leen esa carpeta. Añade Lulo a tu configuración:

- Hyprland: `exec-once = ~/.local/bin/lulo-widget`. Con configuración en Lua, `hl.exec_cmd(os.getenv("HOME") .. "/.local/bin/lulo-widget")` dentro de `hl.on("hyprland.start", function() ... end)`.
- Sway: `exec ~/.local/bin/lulo-widget`.

### Wayland

Wayland no deja que una aplicación elija dónde va su ventana ni que se quede siempre encima, así que Lulo se abre por XWayland, donde sí puede.

- **Hyprland:** Lulo añade solo sus reglas de ventana al arrancar (flotante, visible en todos los escritorios, sin borde ni sombra). Si recargas la configuración de Hyprland se pierden; vuelve a abrir Lulo o ponlas en tu configuración para la clase `lulo`.
- **Sway:** añade `for_window [class="lulo"] floating enable, sticky enable, border none`.
- **GNOME y KDE:** funciona sin cambios.

Con `LULO_WAYLAND=1` Lulo se queda en Wayland nativo; entonces la posición y el «siempre encima» dependen de las reglas de tu compositor.

### Diferencias con Windows

- Los ojos del pulpo no siguen el ratón cuando está fuera de la media luna.
- El clic en una ficha trae al frente la terminal en Hyprland, o en X11 si tienes `wmctrl`. Dentro de tmux o de otra sesión remota no la encuentra.
- **Esconder en pantalla completa** solo funciona en Hyprland.
- Sin sonido con «No molestar» activo en swaync, dunst o mako.

Los ajustes están en `~/.local/share/lulo/widget.json` y los estados en `~/.local/state/claude-status`.

### Actualizar y desinstalar

Para actualizar, descarga la nueva versión y vuelve a ejecutar `./lulo/install.sh`. Cierra el widget, lo reemplaza y nunca duplica los hooks.

Para desinstalar, ejecuta `./lulo/install.sh --uninstall`. Quita los hooks de `settings.json` (dejando el resto como estaba), los programas, el acceso del menú y el inicio automático. Después puedes borrar `~/.local/share/lulo` y `~/.local/state/claude-status`.

## Por qué sale el aviso de Windows

Lulo no está firmado con un certificado de editor, así que Windows lo muestra como de «Editor desconocido» y SmartScreen avisa la primera vez que abres el instalador. Es normal en programas pequeños y gratuitos. Si prefieres revisarlo antes, el código completo está en este repositorio y los programas de cada versión se compilan en GitHub Actions a partir de él.
