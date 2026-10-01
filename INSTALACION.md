# Instalar Lulo

Lulo es una media luna pequeña pegada al borde de arriba de la pantalla que muestra en vivo qué hace cada sesión de Claude Code en tu PC.

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
- **Clic derecho:** Iniciar con Windows, Avisos de Windows, Sonidos, Esconder en pantalla completa, Pantalla, Posición y Cerrar Lulo. Sobre una sesión inactiva, también Quitar de la lista.
- **Volver a abrirla:** busca «Lulo» en el menú Inicio.

Los ajustes están en `%LOCALAPPDATA%\Lulo\widget.json`. Casi todos se cambian desde el clic derecho; estos solo desde el archivo:

- `"animate": false` deja el pulpo quieto y gasta menos.
- `"inactive_minutes"` (5 por defecto): minutos sin actividad antes de marcar una sesión como «Inactiva».
- `"forget_hours"` (12 por defecto): horas sin actividad antes de que Lulo olvide la sesión.
- `"new_session_sound": true` suena una burbuja cuando abres una sesión nueva.

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

## Por qué sale el aviso de Windows

Lulo no está firmado con un certificado de editor, así que Windows lo muestra como de «Editor desconocido» y SmartScreen avisa la primera vez que abres el instalador. Es normal en programas pequeños y gratuitos. Si prefieres revisarlo antes, el código completo está en este repositorio y los programas de cada versión se compilan en GitHub Actions a partir de él.
