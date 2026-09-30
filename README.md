# Lulo

Widget para Windows, siempre visible, que muestra en tiempo real el estado de todas tus sesiones de Claude Code: pensando, editando, en el bash, leyendo, con un subagente, esperando permiso o terminada.

Funciona con [hooks de Claude Code](https://code.claude.com/docs/en/hooks): cada evento ejecuta `lulo-hook.exe`, que escribe el estado de la sesión en `%LOCALAPPDATA%\claude-status\<session_id>.json`. `lulo-widget.exe` vigila esa carpeta y muestra una fila por sesión.

## Estado del proyecto

- [x] Fase 1: binario del hook e instalación en `settings.json`
- [x] Fase 2: widget básico que lista las sesiones
- [x] Fase 3: colores, iconos, posición y arrastre
- [x] Fase 4: sesiones inactivas y pulido

## Instalar el hook

1. Descarga el artefacto `lulo-windows-x64` del último build de CI (trae `lulo-hook.exe` y `lulo-widget.exe`) o compílalos con `cargo build --release`.
2. Haz doble clic en `lulo-hook.exe`. Se copia a `%LOCALAPPDATA%\Lulo\lulo-hook.exe` y añade los hooks a `~/.claude/settings.json` sin tocar el resto del archivo, guardando antes una copia (`settings.json.lulo-backup-<fecha>`). Puedes repetirlo cuando quieras: nunca duplica entradas. Si `lulo-widget.exe` está en la misma carpeta, también lo copia y lo abre.
3. Reinicia las sesiones de Claude Code abiertas para que carguen los hooks.

Desde una terminal también funcionan `lulo-hook.exe install`, `lulo-hook.exe uninstall` y `lulo-hook.exe status-dir` (carpeta donde escribe los estados).

## Abrir el widget

Haz doble clic en `lulo-widget.exe`. Aparece una ventana pequeña, sin bordes, semitransparente y siempre encima, fuera de la barra de tareas. Tiene una fila por sesión con icono y color según el estado, el proyecto, el detalle (archivo, comando…) y hace cuánto fue el último evento. La altura se ajusta al número de sesiones.

- **Mover:** arrástrala desde cualquier punto. La posición se recuerda.
- **Menú (clic derecho):** "Iniciar con Windows" y "Cerrar Lulo".
- **Inactiva:** una sesión sin eventos durante 5 minutos pasa a "Inactiva" y baja al final de la lista. "Esperando" nunca pasa a inactiva, porque necesita que respondas.
- **Sesiones huérfanas:** si cierras una terminal sin `/exit`, Claude Code no envía `SessionEnd`. El widget borra esos archivos tras 12 horas sin eventos.
- Solo se redibuja cuando cambia un archivo de estado, más un refresco cada 15 s para los tiempos.

Los dos umbrales se cambian en `%LOCALAPPDATA%\Lulo\widget.json`:

```json
{ "inactive_minutes": 5, "forget_hours": 12, "x": 1500, "y": 40 }
```

## Archivo de estado

```json
{"session_id":"0b6c…","project":"Lulo","cwd":"C:\\dev\\Lulo","state":"editing","detail":"main.rs","event":"PreToolUse","ts":1759256718}
```

`ts` son segundos Unix del último evento. `detail` es una pista corta (archivo, comando, descripción del subagente) o `null`.

| Evento | `state` |
|---|---|
| `SessionStart` | `ready` |
| `UserPromptSubmit`, `PostToolUse`, `PostToolUseFailure`, `SubagentStop` | `thinking` |
| `PreToolUse` Edit, MultiEdit, Write, NotebookEdit | `editing` |
| `PreToolUse` Bash, PowerShell | `bash` |
| `PreToolUse` Read, Grep, Glob, WebFetch, WebSearch | `reading` |
| `PreToolUse` Agent (antes Task) | `subagent` |
| `PreToolUse` cualquier otra herramienta (MCP, etc.) | `tool` |
| `PermissionRequest`; `Notification` de permiso o de espera de input | `waiting` |
| `Stop` | `done` |
| `StopFailure` | `error` |
| `SessionEnd` | borra el archivo |

Los eventos que vienen de dentro de un subagente (traen `agent_id`) solo actualizan `ts`, para que la sesión principal siga mostrando `subagent`. La excepción es `PermissionRequest`, que siempre pasa a `waiting`.

## Diseño del hook

- Se registra en forma "exec" (`command` + `args`): Claude Code lanza el `.exe` directamente, sin Git Bash ni PowerShell de por medio.
- Los hooks son síncronos para que los estados nunca lleguen desordenados. El binario tarda unos pocos milisegundos: lee el stdin, escribe un archivo y sale.
- Escritura atómica (archivo temporal + renombrar), para que el widget nunca lea un JSON a medias.
- Nunca imprime en stdout ni devuelve error: un fallo del hook no puede bloquear a Claude.
- Única dependencia: `serde_json`.

## Desarrollo

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

`LULO_STATUS_DIR` cambia la carpeta de estado (útil en pruebas). Fuera de Windows se usa `~/.local/state/claude-status`.
