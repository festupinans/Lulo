# Instalador y firma

`lulo.iss` es el instalador de Inno Setup 6. Instala Lulo solo para el usuario actual, en `%LOCALAPPDATA%\Lulo`, sin pedir permisos de administrador:

- copia `lulo-hook.exe` y `lulo-widget.exe`,
- ejecuta `lulo-hook.exe install` para añadir los hooks a `~/.claude/settings.json`,
- crea el acceso directo «Lulo» en el menú Inicio,
- opcionalmente marca «Iniciar con Windows» (el mismo valor que usa el menú del widget),
- aparece en **Configuración > Aplicaciones**, desde donde se desinstala: el desinstalador quita los hooks, el inicio automático y la carpeta `claude-status`.

Instalar encima de una versión anterior (también de una instalada con el zip) la actualiza sin duplicar los hooks.

## Compilar en tu PC

```powershell
winget install JRSoftware.InnoSetup
.\installer\build.ps1          # sin firmar
.\installer\build.ps1 -Sign    # firmado (ver abajo)
```

El instalador queda en `target\installer\Lulo-Setup-<versión>.exe`.

## Firma

`sign.ps1` firma con lo que encuentre en variables de entorno. Inno Setup lo llama también para firmar el desinstalador que va dentro del instalador.

### En GitHub Actions: Azure Artifact Signing

El workflow `release.yml` firma solo cuando existen estos secretos del repositorio (Settings > Secrets and variables > Actions):

| Secreto | Qué es |
| --- | --- |
| `ARTIFACT_SIGNING_ENDPOINT` | Endpoint de la región de tu cuenta, por ejemplo `https://eus.codesigning.azure.net/` |
| `ARTIFACT_SIGNING_ACCOUNT` | Nombre de la cuenta de Artifact Signing |
| `ARTIFACT_SIGNING_PROFILE` | Nombre del perfil de certificado (Public Trust) |
| `AZURE_TENANT_ID` | Directory (tenant) ID de la App Registration |
| `AZURE_CLIENT_ID` | Application (client) ID de la App Registration |
| `AZURE_CLIENT_SECRET` | Un client secret de esa App Registration |

La App Registration necesita el rol **Artifact Signing Certificate Profile Signer** sobre la cuenta de firma. Sin los secretos, el workflow compila y publica sin firmar, como hasta ahora.

### En tu PC: un certificado instalado en Windows

Sirve para Certum SimplySign (con SimplySign Desktop conectado), un token USB o cualquier certificado OV/EV:

```powershell
$env:SIGN_CERT_THUMBPRINT = '<huella SHA-1 del certificado>'
$env:SIGN_TIMESTAMP_URL = 'http://time.certum.pl'   # opcional
.\installer\build.ps1 -Sign
```

La huella sale en `certmgr.msc` > Personal > Certificados > doble clic > Detalles > Huella digital. `signtool.exe` viene con el Windows SDK.

### SmartScreen

Firmar quita el «Editor desconocido» y muestra tu nombre, pero desde 2024 ningún certificado (ni EV) quita el aviso azul de SmartScreen desde el primer día: la reputación se gana con descargas firmadas con la misma identidad. Para acelerarlo, envía cada versión nueva a Microsoft en <https://www.microsoft.com/wdsi/filesubmission> como «Software developer».
