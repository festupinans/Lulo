# Instalar Lulo

Lulo es una media luna pequeña pegada al borde de arriba de la pantalla que muestra en vivo qué hace cada sesión de Claude Code en tu PC.

## Qué necesitas

- Windows 10 u 11.
- Claude Code en este PC, en la terminal o en la pestaña Code de la app de escritorio de Claude.

Lulo ve las sesiones que corren en tu PC. Las sesiones en la nube, por SSH, en WSL o de Cowork no aparecen.

## Instalación

1. Abre la [última versión](https://github.com/festupinans/Lulo/releases/latest) y descarga `lulo-windows-x64.zip`.
2. Descomprime el zip. Deja `lulo-hook.exe` y `lulo-widget.exe` juntos en la misma carpeta.
3. Haz doble clic en `lulo-hook.exe`.
   - Si Windows muestra «Windows protegió su PC», pulsa **Más información** y luego **Ejecutar de todas formas**. Sale porque los archivos no están firmados.
   - Se abre una ventana que copia Lulo a `%LOCALAPPDATA%\Lulo`, añade los hooks a `~/.claude/settings.json` (guardando antes una copia del archivo) y abre el widget. Pulsa Enter para cerrarla.
4. Reinicia las sesiones de Claude Code que tengas abiertas. En la app de escritorio basta con cerrarla y volver a abrirla. Desde ese momento cada sesión aparece como un punto en la media luna.
5. Para que Lulo arranque solo al encender el PC, haz clic derecho sobre la media luna y marca **Iniciar con Windows**.

Después de instalar puedes borrar la carpeta descomprimida: Lulo queda en `%LOCALAPPDATA%\Lulo`.

## Uso

- **Plegada:** la media luna con los ojos del pulpo, que cuelga de cabeza, y un punto de color por cada sesión activa. Los ojos siguen a la sesión que más te necesita.
- **Al pasar el ratón:** aparece una ficha por sesión con su proyecto y su estado (pensando, editando, en el bash, esperando permiso, en segundo plano, terminó…).
- **Clic derecho:** Iniciar con Windows y Cerrar Lulo. Sobre una sesión inactiva, también Quitar de la lista.

Los ajustes están en `%LOCALAPPDATA%\Lulo\widget.json`. Por ejemplo, `"animate": false` deja el pulpo quieto.

## Actualizar

1. Clic derecho sobre la media luna y **Cerrar Lulo**.
2. Descarga el zip de la nueva versión y repite los pasos 2 y 3 de la instalación. Nunca duplica los hooks.

## Desinstalar

1. Clic derecho sobre la media luna, desmarca **Iniciar con Windows** y pulsa **Cerrar Lulo**.
2. En una terminal, ejecuta:

   ```
   %LOCALAPPDATA%\Lulo\lulo-hook.exe uninstall
   ```

   Quita los hooks de Lulo de `settings.json` y deja el resto como estaba.
3. Borra las carpetas `%LOCALAPPDATA%\Lulo` y `%LOCALAPPDATA%\claude-status`.

## Si algo no funciona

- **Una sesión no aparece:** reiníciala. Las sesiones que ya estaban abiertas al instalar no cargan los hooks hasta que se reinician.
- **Una sesión se queda en un estado viejo:** pasa a «Inactiva» tras 5 minutos sin eventos y el widget la olvida tras 12 horas.
- **Quieres ver dónde escribe Lulo:** `%LOCALAPPDATA%\Lulo\lulo-hook.exe status-dir` muestra la carpeta con un archivo por sesión.
