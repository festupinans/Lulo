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

Desde una terminal también funcionan `lulo-hook.exe install`, `lulo-hook.exe uninstall` y `lulo-hook.exe status-dir` (carpeta donde escribe los estados). La instalación también configura la línea de estado que alimenta los anillos de uso.

## Abrir el widget

Haz doble clic en `lulo-widget.exe`. Aparece como una isla de vidrio arriba en el centro de la pantalla, siempre encima y fuera de la barra de tareas.

- **Plegada:** una píldora con el pulpo, una frase y un punto de color por sesión. La frase habla de lo que más te necesita: «api-server te espera», «docs tuvo un error», lo que hace la sesión más reciente («Lulo edita style.rs · 2 más»), «Todo listo» o «Sin actividad».
- **El pulpo:** hace la escena de ese estado. Piensa, teclea frente al PC, mira la terminal, busca con lupa, llama a pulpitos ayudantes, espera impaciente, salta al terminar, tiembla con un error y duerme si no hay actividad.
- **Al pasar el ratón:** la isla se abre con una ficha por sesión (pulpo, proyecto, estado y barra de tareas hechas) y debajo un resumen («3 trabajando · 1 te espera · 1 terminó»). Se vuelve a plegar al sacar el ratón.
- **Al pasar sobre una ficha:** debajo aparece qué hace ahora y sus tareas como pasos (hechas, la actual resaltada y las que faltan). Si espera permiso, dice para qué. Si no usa tareas, muestra el pedido.
- **Anillos de uso:** con la isla abierta, a la izquierda, dos anillos alrededor del pulpo. El de color es el uso que te queda en la ventana de 5 horas de tu plan (verde, ámbar desde la mitad, rojo y latiendo al final). El blanco, más fino, es el tiempo que falta para que se restablezca. Ver [Sobre los anillos de uso](#sobre-los-anillos-de-uso).
- **Vidrio:** la isla dibuja su propio vidrio translúcido con bordes redondos suaves (el desenfoque acrylic de Windows rellenaba el rectángulo entero de la ventana). Con `"glass": false` en `widget.json` queda casi opaca.
- **Segundo plano:** si Claude termina su turno pero dejó comandos o subagentes corriendo en segundo plano, la sesión sale «En segundo plano» en vez de «Terminó», hasta que Claude Code avisa que acabaron.
- **Menú (clic derecho):** "Iniciar con Windows" y "Cerrar Lulo".
- **Inactiva:** una sesión sin eventos durante 5 minutos pasa a "Inactiva" y baja al final. "Esperando" nunca pasa a inactiva, porque necesita que respondas.
- **Sesiones huérfanas:** si cierras una terminal sin `/exit`, Claude Code no envía `SessionEnd`. El widget borra esos archivos tras 12 horas sin eventos.
- **Consumo:** el pulpo se anima a 12 cuadros por segundo plegado y a 30 desplegado. Con `"animate": false` queda quieto y el widget solo se redibuja cuando cambia un archivo de estado o el ratón está encima, más un refresco cada 15 s para los tiempos.

La configuración está en `%LOCALAPPDATA%\Lulo\widget.json`:

```json
{ "glass": true, "animate": true, "inactive_minutes": 5, "forget_hours": 12, "account_usage": true }
```

### Sobre los anillos de uso

Los hooks de Claude Code no traen el uso del plan. Lulo lo obtiene de dos formas y usa la más reciente:

- **Línea de estado (terminal):** la instalación pone `lulo-hook.exe statusline` como línea de estado en `settings.json`. Guarda el uso en `claude-status\_usage.json` y muestra en la terminal algo como `Opus · Lulo · 5 h: 34 % usado, reinicia en 2 h 10 min`. Si ya tenías una línea de estado propia, Lulo no la toca y te avisa.
- **Tu cuenta (app de escritorio):** la app no ejecuta la línea de estado, así que el widget consulta el uso a Anthropic cada 5 minutos, igual que `/usage`. Para eso lee el acceso que Claude Code guarda en `~/.claude/.credentials.json`. El acceso solo se envía a Anthropic, nunca se guarda en otro sitio y Lulo no lo renueva: si caducó, espera a que Claude Code lo renueve. Esta consulta no está documentada por Anthropic y puede dejar de funcionar; en ese caso los anillos simplemente no aparecen. Se desactiva con `"account_usage": false` en `widget.json`. Usa el `curl` que trae Windows 10 y 11.

Solo hay datos en los planes Pro y Max. Sin datos, la isla muestra solo las fichas. `lulo-hook.exe uninstall` quita también la línea de estado de Lulo.

### Sobre la lista de tareas

La lista sale de las herramientas de tareas de Claude Code (`TaskCreate`/`TaskUpdate`, o `TodoWrite` en versiones antiguas). En los modelos más nuevos Claude Code no las activa por defecto, así que la ficha muestra el pedido en lugar de las tareas. Para activarlas, arranca Claude Code con la variable de entorno `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` (por ejemplo, `setx CLAUDE_CODE_ENABLE_TODO_TOOLS 1` en Windows y reinicia la terminal).

## Archivo de estado

```json
{"session_id":"0b6c…","project":"Lulo","cwd":"C:\\dev\\Lulo","state":"editing","detail":"main.rs","event":"PreToolUse","ts":1759256718}
```

`ts` son segundos Unix del último evento. `detail` es una pista corta (archivo, comando, descripción del subagente) o `null`. Además, el hook guarda:

- `prompt` y `started`: el último pedido (recortado) y cuándo se envió.
- `steps`: las últimas 15 acciones de ese pedido (`state`, `detail`, `ts`).
- `tasks`: la lista de tareas de Claude (`text`, `status`: `pending`, `in_progress` o `completed`), a partir de `TodoWrite`, `TaskCreated`, `TaskUpdate` y `TaskCompleted`.

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
