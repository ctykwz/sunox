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
web de Suno. Si no hace falta verificar, envía la solicitud sin abrir un navegador. Si Suno exige
un challenge, Sunox utiliza el navegador Chromium correspondiente a la sesión y elimina el perfil
temporal al terminar.

```text
--captcha          Verificar aunque la comprobación inicial no lo solicite
--no-captcha       Desactivar la resolución automática en el navegador
--token <token>    Usar un token de challenge obtenido externamente
```

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
