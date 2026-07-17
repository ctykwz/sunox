<div align="center">

# sunox

**Genera música con IA desde la terminal usando los workflows Web de Suno**

<br />

[![GitHub](https://img.shields.io/badge/GitHub-ctykwz%2Fsunox-181717?style=for-the-badge&logo=github)](https://github.com/ctykwz/sunox)

<br />

[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
&nbsp;
[![Rust](https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
&nbsp;
[![crates.io](https://img.shields.io/crates/v/sunox?style=for-the-badge)](https://crates.io/crates/sunox)
&nbsp;
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen?style=for-the-badge)](https://github.com/ctykwz/sunox/pulls)

---

`sunox` es un binario Rust único que llama directamente a los endpoints Web de Suno. Soporta letras personalizadas, tags de estilo, voice personas, control vocal, sliders de weirdness/style, covers, remasters, cambios de velocidad, extracción de stems y escritura automática de letras al descargar.

**Idiomas:** [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [Français](README.fr.md) | Español

[Instalación](#instalación) | [Inicio rápido](#inicio-rápido) | [Comandos para personas](#comandos-para-personas) | [Comandos para agentes y avanzados](#comandos-para-agentes-y-avanzados) | [Funciones](#funciones) | [Contribuir](#contribuir)

</div>

## Por qué

La interfaz Web de Suno funciona bien para uso manual, pero no está pensada para scripting, leer letras desde archivos, generación por lotes o integrarse en un flujo musical basado en terminal.

`sunox` resuelve eso: autenticación automática desde el navegador, parámetros de generación expuestos como flags CLI, salida tanto legible para humanos como JSON estructurado, y letras sincronizadas integradas automáticamente en los MP3 descargados.

Sunox es un proyecto no oficial y no está afiliado ni respaldado por Suno. Utiliza APIs Web privadas que pueden cambiar sin previo aviso. Cada usuario es responsable de cumplir los términos de Suno, los límites de su cuenta y los derechos aplicables al material generado o subido.

## Instalación

### Cargo

```bash
cargo install sunox
```

Requiere Rust 1.88 o posterior.

### Binarios precompilados

Descarga binarios para macOS, Linux y Windows desde [GitHub Releases](https://github.com/ctykwz/sunox/releases).
Cada release incluye `SHA256SUMS`; `sunox update` verifica el archivo seleccionado antes de instalarlo.

### Autoactualización

```bash
sunox update --check    # ver si hay una versión nueva
sunox update            # instalar la última release
```

Cuando Suno cambie su esquema Web, ejecuta primero `sunox update`. Suele ser más rápido que esperar a que se actualice un gestor de paquetes.

## Inicio rápido

```bash
# 1. Iniciar sesión, con extracción automática desde Chrome / Arc / Brave / Firefox / Edge
sunox login

# 2. Generar desde una descripción natural
sunox "una pista chill lo-fi sobre una mañana lluviosa"

# 3. Generar con control completo
sunox create \
  --title "Weekend Code" \
  --tags "indie rock, guitar, upbeat" \
  --exclude "metal, heavy" \
  --lyrics-file lyrics.txt \
  --vocal male \
  --weirdness 40 \
  --style-influence 65

# 4. Esperar los clip IDs devueltos y luego descargar el audio terminado
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs/

# 5. Añadir un resultado a una playlist
sunox add <clip_id> --to <playlist_id>
```

Para agentes y scripts, empieza con `sunox agent-info --json` y luego llama a los comandos de recursos con `--json`.

## Opciones globales

| Opción | Descripción |
|---|---|
| `--json` | Fuerza salida JSON estructurada; se activa automáticamente cuando stdout se redirige |
| `--quiet` | Reduce mensajes de progreso no esenciales |
| `--parallel` | Permite escrituras Suno concurrentes para la misma cuenta; por defecto se serializan por cuenta |
| `-c key=value` / `--config key=value` | Sobrescribe una configuración para esta ejecución, por ejemplo `-c default_model=v5.5 -c output_dir=./songs`; repetible |
| `-V` / `--version` | Muestra la versión |
| `-h` / `--help` | Muestra ayuda de la orden o suborden |

Las escrituras Suno se serializan por cuenta por defecto. Desactívalo de forma
persistente con `sunox config set serial_mutations false`, para una invocación
con `-c serial_mutations=false`, o para una sola orden con `--parallel`.
Las variables de entorno usan el prefijo `SUNOX_*`, por ejemplo `SUNOX_DEFAULT_MODEL`, `SUNOX_OUTPUT_DIR` y `SUNOX_BROWSER_PATH`.

## Comandos para personas

Para el uso diario normalmente bastan estas entradas:

```text
sunox <prompt>                  Generar desde una descripción simple
sunox create [prompt]           Generar con título, tags, letras, modelo, persona
sunox download <clip_ids>       Descargar canciones terminadas
sunox add <clip_ids> --to <id>  Añadir canciones a una playlist
sunox login                     Configurar auth desde el navegador
sunox logout                    Eliminar la auth local y el perfil login interactivo
sunox doctor                    Diagnosticar configuración y auth
sunox doctor --network          Diagnosticar DNS, TCP y HTTPS (`--strict` devuelve error si hay degradación)
```

## Comandos para agentes y avanzados

`sunox` mantiene disponibles los workflows Suno de bajo nivel para agentes tipo Codex, automatización y depuración. Los agentes deberían preferir `--json` y descubrir el contrato actual con `sunox agent-info --json`.

### Crear y transformar

```text
sunox create              Modo descripción o modo letras personalizadas
sunox lyrics              Generar solo letras, sin consumir credits
sunox clip extend         Continuar un clip desde un timestamp
sunox clip concat         Unir clips en una canción completa
sunox clip cover          Crear una cover con otro estilo o modelo
sunox clip inspire        Generar una canción nueva usando un clip como inspiración libre
sunox clip remaster       Remasterizar con otro modelo
sunox clip speed          Ajustar la velocidad de reproducción
sunox clip reverse        Invertir el audio
sunox clip crop           Recortar a una sección o quitar una sección
sunox clip fade           Añadir fundido de entrada/salida
sunox clip stems          Generar stems desde un clip existente
```

### Explorar e inspeccionar

```text
sunox clip list
sunox clip list --trashed
sunox clip list --liked --public --sort popular
sunox clip search <query>
sunox clip info <id>
sunox clip status <ids>
sunox clip wait <ids>
sunox persona list
sunox persona info <id>
sunox persona clips <id>
sunox playlist list
sunox playlist info <id>
sunox credits
sunox models
```

### Gestionar recursos

```text
sunox download <ids>       MP3 CDN por defecto; --format mp3|m4a|wav|opus es explícito
sunox clip download <ids>  Equivalente avanzado/agent de download
sunox clip upload <file>
sunox clip upload-status <upload_id>
sunox clip delete <ids> -y
sunox clip restore <ids>
sunox clip purge <ids> -y       # elimina canciones de la papelera de forma permanente
sunox clip empty-trash -y       # vacía la papelera de forma irreversible
sunox clip like <ids>
sunox clip dislike <ids>
sunox clip set <id>
sunox clip set <id> --image-file ./cover.png
sunox clip set <id> --image-url <cover_url>
sunox clip set <id> --remove-video-cover
sunox clip publish <ids>
sunox add <clip_ids> --to <playlist_id>
sunox playlist add <playlist_id> <clip_ids>
sunox playlist remove <playlist_id> <clip_ids>
sunox playlist publish <playlist_id>
sunox playlist reorder <playlist_id> --clip-id <clip_id> --index 0
sunox playlist save <playlist_id>
sunox playlist unsave <playlist_id>
sunox playlist delete <playlist_id> -y
```

### Configuración y auth

```text
sunox login
sunox logout
sunox auth
sunox config
sunox doctor
sunox doctor --network
sunox agent-info
sunox install-skill
sunox update
```

## Funciones

Las funciones de Studio están fuera del alcance de este CLI.

### Auth sin fricción

```bash
sunox login
```

`sunox login` primero intenta leer la cookie Clerk desde Chrome, Arc, Brave, Firefox o Edge. Si esa extracción funciona, Sunox guarda la fuente del navegador y los ajustes públicos disponibles, como los idiomas aceptados, pero no fabrica un user-agent solo a partir de la etiqueta del navegador. Si esa extracción falla, abre un perfil de navegador dedicado de Sunox y compatible con Chrome/Edge, y espera a que inicies sesión en Suno allí. La sesión Clerk capturada se intercambia por un JWT y se guarda localmente para futuros refrescos. El login interactivo también captura user-agent e idiomas aceptados; las peticiones API derivan los client hints de Chromium desde el user-agent seleccionado, envían los headers de fetch metadata del navegador y hacen fallback campo por campo cuando no hay valores reales.

Las credenciales se guardan en un JSON local, no en el almacén de credenciales del sistema. En Unix el archivo se crea con modo `0600`; en Windows Sunox depende de la ACL del usuario sobre el directorio de configuración. Los valores de `--cookie` y `--jwt` pueden aparecer en el historial del shell y en la lista de procesos, por lo que conviene usar `sunox login` o `--cookie-stdin` / `--jwt-stdin`, y no incluir credenciales en logs, prompts, archivos del proyecto ni commits.

Métodos de autenticación:

1. `sunox login`: extracción automática del navegador con fallback interactivo en Chrome/Edge, recomendada.
2. `printf '%s' "$SUNOX_COOKIE_INPUT" | sunox auth --cookie-stdin`: leer la cookie desde stdin.
3. `printf '%s' "$SUNOX_JWT_INPUT" | sunox auth --jwt-stdin`: leer el JWT desde stdin.
4. `sunox auth --refresh`: forzar un JWT nuevo desde la sesión Clerk guardada.

`sunox logout` elimina las credenciales locales, el perfil de login interactivo y el perfil captcha heredado.

### Parámetros de generación

| Parámetro | Uso | Valores |
|---|---|---|
| `--title` | Título de la canción | hasta 100 caracteres |
| `--tags` | Dirección de estilo | Límite del modelo/cuenta; consulta `sunox models --json` |
| `--enhance-tags` | Mejorar los tags con el flujo tag upsample de Suno Web antes de enviar | opt-in explícito |
| `--exclude` | Estilos a evitar | Límite del modelo/cuenta; consulta `sunox models --json` |
| `--lyrics` / `--lyrics-file` | Letras personalizadas | `max_lengths.gpt_description_prompt` |
| `--prompt` | Prompt del modo descripción | `max_lengths.prompt` |
| `--model` | Versión del modelo | v5.5, v5, v4.5+, v4.5-all, v4.5, v4, v3.5, v3, v2 |
| `--vocal` | Género vocal | male, female |
| `--persona` | ID de voice persona | UUID de la voz en Suno |
| `--weirdness` | Nivel experimental | 0-100 |
| `--style-influence` | Fidelidad al estilo | 0-100 |
| `--instrumental` | Instrumental sin voces | flag |

### Voice personas

```bash
sunox persona list
sunox persona info <persona_id>
sunox persona create <clip_id> --name "My Voice" --description "Warm lead vocal"
sunox create --persona <persona_id> --title "My Song" --tags "pop" --lyrics "[Verse]\nHello world"
```

También puedes publicar, despublicar, marcar como favorita, eliminar, restaurar o purgar una persona:

```bash
sunox persona publish <persona_id>        # solo si quieres hacerla publica
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id>
sunox persona purge <persona_id> -y       # eliminacion permanente
```

### Playlists

```bash
sunox playlist list
sunox playlist create --name "Release candidates" --description "Tracks to review"
sunox add <clip_id_1> <clip_id_2> --to <playlist_id>
sunox playlist remove <playlist_id> <clip_id_1>
sunox playlist publish <playlist_id> --private
sunox playlist reorder <playlist_id> --clip-id <clip_id> --index 0
```

### Transformaciones de clips

```bash
# Estos comandos pueden devolver un clip submitted/processing; espera antes de continuar
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip inspire <clip_id> --title "New Song" --tags "garage pop" --lyrics-file lyrics.txt
sunox clip remaster <clip_id> --model v5.5 --variation subtle # subtle, normal o high
sunox clip speed <clip_id> --multiplier 0.94
sunox clip reverse <clip_id>
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/

# crop/fade ya esperan a que el clip resultante esté complete; no requieren otro wait
sunox clip crop <clip_id> --start 12.5 --end 74.0
sunox clip crop <clip_id> --start 30.0 --end 45.0 --remove-section
sunox clip fade <clip_id> --in 2.0 --out 78.5
```

### Descargar e integrar letras

Al descargar MP3, `sunox` escribe automáticamente:

- **USLT**: letras simples.
- **SYLT**: letras sincronizadas palabra por palabra.

```bash
sunox download <id1> <id2> --output ./songs/

# Usa --force solo para reemplazar explícitamente un archivo existente
sunox download <id1> --output ./songs/ --force
sunox download <id1> --format wav --output ./songs/
sunox download <id1> --video --output ./videos/
```

Los archivos usan el formato `title-slug-clipid8.<ext>`. Los directorios de salida se crean automáticamente y los archivos existentes se conservan salvo con `--force`.

### Subir audio

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
sunox clip upload-status <upload_id> --json  # solo lectura; no repite la mutación
```

## Modelos

| Versión | Codename | Descripción |
|---|---|---|
| auto | respuesta de la cuenta | Valor CLI predeterminado; elige el modelo utilizable por defecto de la cuenta |
| v5.5 | chirp-fenix | Generación más reciente; fallback solo si billing no está disponible |
| v5 | chirp-crow | Generación anterior |
| v4.5+ | chirp-bluejay | Capacidades ampliadas |
| v4.5-all | chirp-auk-turbo | Opción gratuita cuando la cuenta la ofrece |
| v4.5 | chirp-auk | Versión estable |
| v4 | chirp-v4 | Versión antigua |
| v3.5 | chirp-v3-5 | Versión antigua |
| v3 | chirp-v3-0 | Versión antigua |
| v2 | chirp-v2-xxl-alpha | Versión antigua |

Modelos de remaster: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

La disponibilidad, el modelo predeterminado y los límites dependen de la cuenta. `default_model=auto` elige directamente el modelo utilizable por defecto desde `/api/billing/info/`; `sunox models --json` expone los mismos datos de la cuenta para inspección. Los modelos explícitos se validan con `can_use` y `max_lengths` cuando billing está disponible; v5.5 solo se usa como fallback si esa lectura falla.

## Salida amigable para agentes

- Todas las órdenes soportan `--json`.
- Cuando stdout está redirigido, se activa JSON automáticamente.
- El progreso y los errores van a stderr para no contaminar el JSON.
- Las escrituras Suno se serializan por cuenta por defecto; no uses `sunox config set serial_mutations false`, `-c serial_mutations=false` ni `--parallel` salvo que el usuario permita explícitamente escrituras concurrentes en la misma cuenta.
- Para una inspección de audio normal, usa el medio existente del clip: `sunox clip info <id> --json` expone `audio_url` y también `attribution`, `comments`, `direct_children_count` y `similar_clips`; si falla una lectura suplementaria sin ser un error de autenticación ni de límite de tasa, el clip base sigue devolviéndose con `supplemental_errors`. Los errores de autenticación y límite de tasa abortan normalmente. Por defecto, `sunox clip download` descarga el MP3 CDN de `audio_url` e incrusta letras; `--format mp3|m4a|wav|opus` solicita explícitamente el formato oficial de Suno y `--video` usa `clip.video_url` cuando existe. `sunox clip stems` es extracción de stems basada en generación, distinta del export Pro Get Stems de Suno Web. Los agentes solo deben solicitar un formato explícito, stems o video cuando el usuario lo pida. `--quiet` elimina el progreso de descarga y la salida de estado ordinaria. Si una descarga por lotes devuelve `partial_download`, revisa `error.details.succeeded`, `error.details.failed` y `error.details.not_attempted_clip_ids`, y vuelve a intentar solo los ID necesarios. Si `playlist remove` o una operación de publicación/reacción sobre varios clips devuelve `partial_mutation`, revisa `error.details.succeeded_clip_ids`, `error.details.failed` y `error.details.not_attempted_clip_ids` antes de reintentar.
- La creación/edición de playlists, la subida de imágenes locales, las portadas y la subida de audio son flujos de varios pasos. Un fallo devuelve `partial_mutation` con identificadores, `completed_steps`, `failed.step/code/message` y `recovery`. Sigue el comando estructurado solo si `recovery.resumable=true` y nunca repitas una mutación marcada false. El audio se transmite por streaming y los metadatos se consultan hasta que los campos solicitados sean visibles. `clip upload-status` es estrictamente de solo lectura.
- Sin una petición explícita del usuario, no publiques recursos, no fuerces `--captcha`, no imprimas material de autenticación y no ejecutes comandos destructivos; esos comandos requieren `-y/--yes`.
- Las respuestas de error incluyen una acción sugerida.

```bash
sunox clip list | jq '.data.clips[0].title'
sunox clip list --liked --public --sort popular --json
sunox agent-info --json
```

Códigos de salida semánticos:

| Código | Significado | Acción sugerida |
|---|---|---|
| 0 | Éxito | Continuar |
| 1 | Error runtime, endpoint Web, mutación parcial o descarga parcial | Revisar `error.code` y `error.details` antes de reintentar |
| 2 | Error de configuración | Corregir la config, no reintentar a ciegas |
| 3 | Error de autenticación | Ejecutar `sunox login` |
| 4 | Rate limit | Esperar 30-60 segundos |
| 5 | Recurso no encontrado | Verificar el ID |

## Instalar como skill para coding agents

```bash
# Codex / Trae CLI
sunox install-skill

# Claude Code
sunox install-skill --target claude

# Cursor
sunox install-skill --target cursor
```

## Notas de implementación

Los flujos generate, describe, persona, cover y extend reutilizan `/api/generate/v2-web/` de Suno Web. El body custom create fue recapturado el 30 de junio de 2026: las letras personalizadas se envían en `gpt_description_prompt`, mientras `prompt` queda vacío; con un challenge token resuelto también se envía `token_provider: 1`. Sunox rellena `metadata.user_tier` desde el `plan.id` de `/api/billing/info/` de la cuenta actual cuando está disponible, y cae al valor vacío compatible con Web si no puede leerlo. Con `--enhance-tags`, Sunox llama primero a `/api/prompts/upsample`, coloca los tags y el `request_id` devueltos en `metadata.last_tags_generation` y marca `override_fields=["tags"]`; el campo `personalization_enabled` sigue la forma del submit Web capturado. Sin ese flag, no envía `metadata.last_tags_generation`. Instrumental create también usa custom mode: con `sunox create --instrumental <prompt>`, el prompt se integra en los style tags y el campo `prompt` enviado queda vacío, igual que en la petición Web recapturada en `15suno-labs-nostudio-20260630.har`. `task: "playlist_condition"` también fue capturado, pero pertenece a un flujo inspiration separado que pone letras en `prompt`, por lo que no debe reutilizar las reglas del custom create estándar. Extend lee el clip fuente antes de enviar; si `GET /api/feed/?ids` omite los metadatos de estilo fuente, Sunox busca el título fuente en feed/v3 y fusiona solo los metadatos del clip id exacto. `title` usa el título fuente salvo que se pase `--title`; `tags`, `negative_tags` y `metadata.make_instrumental` se heredan cuando están disponibles. Usa `--tags`, `--exclude`, `--instrumental` o `--no-instrumental` para sobrescribir esos valores. `clip list` usa `POST /api/feed/v3` y expone filtros de consulta como `--liked`, `--public`, `--upload`, `--cover`, `--extend` y `--sort popular`; no es un flujo de library sync. Remaster usa `/api/generate/upsample`, y speed adjust usa `/api/clips/adjust-speed/`. Por defecto, `sunox` no envía challenge token; si Suno devuelve required y hay material Clerk para refrescar, Sunox refresca el JWT una vez y repite el preflight antes de pedir `--token <solved>` o un `--captcha` explícito. Los bodies de cover generation y concat edit todavía necesitan una captura live nueva. Las mutaciones de playlists están implementadas con evidencia de bundle/live y tests de contrato de endpoint; `playlist remove` envía un clip por petición porque los lotes grandes pueden devolver Suno 500.

`sunox clip inspire` implementa el `task=playlist_condition` capturado en producción: una sola fuente, tag upsample real y letras en `prompt`. No expone variantes multi-source ni instrumentales que no hayan sido capturadas. Las variables de entorno públicas usan el prefijo `SUNOX_*`.

## Contribuir

1. Crea una rama: `git checkout -b feature/your-idea`
2. Haz los cambios y ejecuta `cargo test`
3. Abre una PR

Son especialmente bienvenidos los tests de integración con `assert_cmd` y el soporte de almacenamiento de secretos mediante OS keychain / Secret Service / CredMan.

## License

MIT, ver [LICENSE](LICENSE).
