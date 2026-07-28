# sunox

`sunox` es una herramienta no oficial de línea de comandos para usar Suno desde una terminal.
Está escrita en Rust y se distribuye como un único binario. Permite crear canciones, descargar
resultados, administrar playlists y personas de voz, hacer covers y remasters, editar audio y
subir archivos.

[![crates.io](https://img.shields.io/crates/v/sunox)](https://crates.io/crates/sunox)
[![CI](https://github.com/ctykwz/sunox/actions/workflows/ci.yml/badge.svg)](https://github.com/ctykwz/sunox/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) ·
[Français](README.fr.md) · Español

> [!WARNING]
> Sunox no está afiliado a Suno ni cuenta con su aprobación. Utiliza API privadas de la aplicación
> web, que pueden cambiar sin previo aviso. Cada usuario debe cumplir las condiciones de Suno, los
> límites de su cuenta y los derechos aplicables al material generado o subido.

## Qué permite hacer

- Crear una canción a partir de una descripción, letras propias, estilos, una persona de voz o una
  indicación instrumental.
- Esperar a que termine la generación y descargar MP3, M4A, WAV, Opus o vídeo.
- Consultar, buscar, editar, publicar, eliminar y restaurar canciones.
- Crear un cover, extender, unir, remasterizar, invertir, recortar, aplicar fades, cambiar la
  velocidad o generar stems.
- Administrar playlists y personas de voz, y subir audio local o portadas.
- Mostrar tablas en la terminal o JSON estable para scripts y agentes de programación.

Las funciones de Suno Studio quedan fuera del alcance del proyecto.

## Instalación

Con Rust 1.88 o una versión posterior:

```bash
cargo install sunox
```

También hay binarios preparados para macOS, Linux y Windows en
[GitHub Releases](https://github.com/ctykwz/sunox/releases). No llevan una firma comercial de
Apple o Windows, por lo que el sistema puede mostrar el aviso habitual para software descargado.
Cada versión incluye `SHA256SUMS`, y `sunox update` verifica el archivo antes de instalarlo.

## Inicio de sesión

Primero inicia sesión en suno.com desde un navegador compatible y después ejecuta:

```bash
sunox login
```

Sunox busca una sesión reutilizable en Chrome, Edge, Brave, Arc, Chromium o Firefox. Si no
encuentra ninguna, abre un perfil independiente del navegador para completar el acceso de forma
interactiva.

Las credenciales se guardan en el directorio de configuración local de Sunox. No pases cookies o
JWT directamente en la línea de comandos: pueden quedar visibles en el historial o en la lista de
procesos. En un servidor sin interfaz gráfica, usa `--cookie-stdin` o `--jwt-stdin`.

```bash
sunox doctor
sunox credits
```

## Crear y descargar una canción

Para empezar basta con una descripción breve:

```bash
sunox "electrónica ambiental cálida, pulso lento y sintetizadores suaves"
```

Para usar letras propias y ajustar la generación:

```bash
sunox create \
  --title "Night Drive" \
  --tags "dream pop, synth, female vocal" \
  --exclude "metal, aggressive" \
  --lyrics-file lyrics.txt \
  --weirdness 35 \
  --style-influence 70
```

### Modos instrumentales

Elige un solo modo. `--instrumental` no se puede combinar con `--lyrics` ni `--lyrics-file`:

- Para un instrumental sin letra y sin estructura interna controlada, usa solo `--instrumental`.
- Para controlar secciones, ritmo, puntos de montaje o arreglo, omite `--instrumental` y usa un
  archivo cuya primera línea sea `[Instrumental]`. Todas las demás líneas no vacías deben quedar
  entre corchetes, sin texto que pueda cantarse.

Después de generar, ejecuta `sunox clip timed-lyrics <clip_id> --json`. Descarta la versión si
aparece cualquier palabra alineada no vacía con `success=true`.

Una solicitud de generación suele devolver dos ID de clip. Espera a que terminen y descarga las
versiones que quieras conservar:

```bash
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs
```

Sin indicar un formato, Sunox descarga el MP3 ya disponible en el CDN e incorpora las letras
normales y sincronizadas en las etiquetas ID3 cuando existen. Usa `--format mp3|m4a|wav|opus`
solo si necesitas la conversión de Suno, o `--video` para descargar un vídeo disponible.

## Comandos habituales

```text
sunox <descripción>                 Crear a partir de una descripción
sunox create [descripción]          Crear con todos los ajustes
sunox lyrics                        Generar solo letras

sunox clip list                     Listar tus canciones
sunox clip search <búsqueda>        Buscar canciones
sunox clip info <id>                Ver los detalles de una canción
sunox clip wait <ids>               Esperar a que termine la generación
sunox download <ids>                Descargar canciones terminadas

sunox clip cover <id>               Crear un cover
sunox clip extend <id>              Extender una canción
sunox clip concat <ids>             Unir varios clips
sunox clip remaster <id>            Remasterizar
sunox clip speed <id>               Cambiar la velocidad
sunox clip reverse <id>             Invertir el audio
sunox clip crop <id>                Conservar o eliminar un fragmento
sunox clip fade <id>                Añadir un fade
sunox clip stems <id>               Generar stems

sunox playlist list                 Listar playlists
sunox playlist create               Crear una playlist
sunox add <clip_ids> --to <id>      Añadir canciones a una playlist

sunox persona list                  Listar personas de voz
sunox persona create <clip_id>      Crear una persona desde una canción

sunox clip upload <archivo>         Subir audio local
sunox models                        Ver los modelos disponibles
sunox doctor --network              Comprobar DNS, TCP y HTTPS
sunox update                        Instalar la última versión de GitHub
```

Consulta `sunox --help` o `sunox <comando> --help` para ver todas las opciones.

## Verificación de generación

Antes de enviar una solicitud de generación, Sunox ejecuta la misma comprobación que la aplicación
web de Suno. Si no hace falta verificar, envía la solicitud directamente y no abre ningún
navegador. Si Suno exige un challenge, primero pide a la extensión opcional Browser Bridge que
ejecute el widget invisible dentro del perfil habitual de Chrome. Mientras está inactiva, la
extensión solo mantiene su listener local. Cuando hace falta verificar, usa el documento offscreen
invisible de Chrome y coloca dentro un único iframe de `suno.com` ligado a un nonce. El iframe
conserva un viewport normal para el proveedor, pero Chrome no crea pestañas, ventanas emergentes,
ventanas minimizadas ni otro proceso de navegador. Solo el iframe de primer nivel propiedad de la
extensión puede conectarse; una navegación, recarga, desconexión o identidad inesperada elimina el
iframe y falla de forma segura. El iframe también se elimina tras el token o el error final, sin
ninguna alternativa visible. Este comportamiento es compatible tanto con macOS como con Windows.

Si el Bridge no responde, el modo predeterminado `auto` solo recurre a un navegador de la familia
Chromium instalado cuando no hay un emparejamiento del Bridge configurado. Una vez instalado el
Bridge, `auto` falla de forma segura en vez de iniciar un proceso de navegador separado. Usa
`challenge_browser=isolated` explícitamente cuando aceptes esa alternativa independiente.

### Instalar Browser Bridge en macOS o Windows

Browser Bridge viene incluido en el binario de Sunox: no hace falta descargar un ZIP ni usar Chrome
Web Store. La configuración es la misma en macOS y Windows:

1. Extrae la extensión incluida y anota el directorio que muestra el comando:

   ```bash
   sunox install-browser-extension
   ```

2. En el mismo perfil de Chrome en el que usas Suno, abre `chrome://extensions`.
3. Activa **Modo de desarrollador**, selecciona **Cargar descomprimida** y elige exactamente el
   directorio indicado por Sunox. En macOS, pulsa `Shift+Command+G` en el selector de carpetas y
   pega la ruta, ya que `~/Library` está oculta de forma predeterminada. En Windows, pega la ruta
   indicada en la barra de direcciones del selector.
4. Mantén activada la extensión. No es necesario conservar ninguna pestaña de Suno abierta.

Comprueba el emparejamiento sin crear una canción, ejecutar un challenge ni consumir créditos:

```bash
sunox doctor --browser-bridge
```

La extensión permanece instalada tras reiniciar el navegador. Después de una actualización de
Sunox que cambie el Bridge, actualiza sus archivos y recárgala en Chrome:

```bash
sunox install-browser-extension --force
```

El comando compara primero el paquete generado con los archivos extraídos. Pulsa **Recargar** en la
tarjeta Sunox Browser Bridge solo cuando informe `updated` o `reload_required`; si informa
`already_current`, no hace falta recargarla en Chrome. Reiniciar el ordenador o Chrome por sí solo
nunca exige reinstalar ni recargar el Bridge. Tampoco es necesario recargar ninguna página de
Suno. El comando elige el directorio de aplicación correcto de cada usuario tanto en macOS como en
Windows; no muevas ni borres ese directorio mientras Chrome use la extensión sin empaquetar.

```text
--captcha          Verificar aunque la comprobación inicial no lo solicite
--no-captcha       Desactivar la resolución automática en el navegador
--token <token>    Usar un token de challenge obtenido externamente
```

Configura `challenge_browser` como `auto` (predeterminado), `existing` (exige el Bridge y nunca
inicia un proceso de navegador separado) o `isolated` (siempre usa el navegador temporal). Puedes
anularlo en un único comando con `-c challenge_browser=existing`. El nombre `existing` se conserva
por compatibilidad de configuración: ahora significa «usar el Bridge instalado en el perfil de
Chrome existente». El Bridge crea y elimina automáticamente un iframe offscreen vinculado a un
nonce; no abre ninguna pestaña ni ventana. Un Bridge ya configurado que esté ausente u obsoleto se comunica como error en
vez de abrir otro navegador o recurrir a un contexto visible.
En modo `auto`, Sunox puede abrir el respaldo aislado solo si no hay un emparejamiento del Bridge
configurado. Si el Bridge instalado está desactivado, obsoleto o no es accesible, falla de forma
segura; usa `isolated` explícitamente para permitir un proceso de navegador separado.

Para ejecuciones desatendidas que no deban añadir una pestaña de Suno a la ventana activa ni abrir
otro proceso de navegador, instala Browser Bridge y omite `--no-captcha`. Tanto `auto` como
`challenge_browser=existing` fallan de forma segura si el Bridge no está disponible; `existing`
además exige el Bridge aunque no se haya configurado ningún emparejamiento. Si el Bridge no está
instalado o no conoces su estado, conserva `--no-captcha`: un challenge necesario se detendrá antes
del envío. Sin un Bridge configurado, omitir `--no-captcha` en el modo predeterminado `auto` todavía
permite el respaldo mediante navegador aislado.

Instalar el Bridge autoriza permanentemente a Sunox a ejecutar challenges en el contexto efímero que
administra automáticamente; no hace falta pedir permiso aparte en cada generación. Solicitudes como
«no dejar una pestaña de Suno abierta», «sin navegador nuevo» o «sin captcha visible» permiten el
Bridge instalado y no significan `--no-captcha`; `challenge_browser=existing` sigue siendo la
anulación explícita que usa solo el Bridge. Conserva `--no-captcha` pese a tener el Bridge instalado
únicamente si se prohíben explícitamente todos los mecanismos de challenge, incluido el Bridge, o
si se solicita esa opción exacta.

## JSON y automatización

Todos los comandos aceptan `--json`. La salida también cambia a JSON automáticamente al conectarla
a un pipe:

```bash
sunox clip list --json
sunox clip list | jq '.data.clips[0].title'
sunox agent-info --json
```

Los errores tienen códigos estables y estados de salida distintos de cero. Si una operación por
lotes falla a medias, la respuesta separa los elementos completados, fallidos y no ejecutados para
que solo sea necesario reintentar lo pendiente.

El paquete también incluye un Skill de uso para agentes de programación:

```bash
sunox install-skill                 # Codex
sunox install-skill --target claude
sunox install-skill --target cursor
```

## Configuración y seguridad

```bash
sunox config show
sunox config set output_dir ./songs
sunox config set default_model auto
```

`-c key=value` solo modifica una ejecución. Las variables de entorno usan el prefijo `SUNOX_*`.

Las escrituras de una misma cuenta se ejecutan en serie de forma predeterminada para evitar
conflictos. `--parallel` desactiva esa protección durante un comando; úsalo únicamente cuando las
escrituras simultáneas sean intencionadas.

Algunos comandos consumen créditos o cambian recursos remotos. Las canciones, playlists y personas
nuevas permanecen privadas salvo que se solicite expresamente su publicación. Las operaciones
irreversibles requieren `-y` o `--yes`.

## Desarrollo

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Crea una rama desde `main` y abre una Pull Request para proponer cambios.

## Licencia

[MIT](LICENSE)
