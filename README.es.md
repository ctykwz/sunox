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

## Instalación

### Cargo

```bash
cargo install sunox
```

### Binarios precompilados

Descarga binarios para macOS, Linux y Windows desde [GitHub Releases](https://github.com/ctykwz/sunox/releases).

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
| `-c key=value` / `--config key=value` | Sobrescribe una configuración para esta ejecución, por ejemplo `-c default_model=v5.5 -c output_dir=./songs`; repetible |
| `-V` / `--version` | Muestra la versión |
| `-h` / `--help` | Muestra ayuda de la orden o suborden |

## Comandos para personas

Para el uso diario normalmente bastan estas entradas:

```text
sunox <prompt>                  Generar desde una descripción simple
sunox create [prompt]           Generar con título, tags, letras, modelo, persona
sunox download <clip_ids>       Descargar canciones terminadas
sunox add <clip_ids> --to <id>  Añadir canciones a una playlist
sunox login                     Configurar auth desde el navegador
sunox logout                    Eliminar la auth local
sunox doctor                    Diagnosticar configuración y auth
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
sunox clip remaster       Remasterizar con otro modelo
sunox clip speed          Ajustar la velocidad de reproducción
sunox clip stems          Extraer stems de voz e instrumentos
```

### Explorar e inspeccionar

```text
sunox clip list
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
sunox download <ids>
sunox clip download <ids>
sunox clip upload <file>
sunox clip delete <ids>
sunox clip restore <ids>
sunox clip like <ids>
sunox clip dislike <ids>
sunox clip set <id>
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
sunox agent-info
sunox install-skill
sunox update
```

## Funciones

### Auth sin fricción

```bash
sunox login
```

`sunox` lee la cookie Clerk desde Chrome, Arc, Brave, Firefox o Edge, la intercambia por un JWT, guarda una sesión local renovable y refresca automáticamente los JWT caducados.

Métodos de autenticación:

1. `sunox login`: extracción automática del navegador, recomendada.
2. `sunox auth --cookie <cookie>`: pegar una cookie manualmente en servidores headless.
3. `sunox auth --jwt <token>`: JWT directo, normalmente válido alrededor de 1 hora.
4. `sunox auth --refresh`: forzar un JWT nuevo desde la sesión Clerk guardada.

### Parámetros de generación

| Parámetro | Uso | Valores |
|---|---|---|
| `--title` | Título de la canción | hasta 100 caracteres |
| `--tags` | Dirección de estilo | por ejemplo `"pop, synths, upbeat"` |
| `--exclude` | Estilos a evitar | por ejemplo `"metal, heavy, dark"` |
| `--lyrics` / `--lyrics-file` | Letras personalizadas | soporta secciones como `[Verse]` |
| `--prompt` | Prompt del modo descripción | hasta 500 caracteres |
| `--model` | Versión del modelo | v5.5, v5, v4.5+, v4.5, v4, v3.5, v3, v2 |
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
sunox persona publish <persona_id>
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id> -y
sunox persona purge <persona_id> -y
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
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip remaster <clip_id> --model v5.5
sunox clip speed <clip_id> --multiplier 0.94
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/
```

### Descargar e integrar letras

Al descargar MP3, `sunox` escribe automáticamente:

- **USLT**: letras simples.
- **SYLT**: letras sincronizadas palabra por palabra.

```bash
sunox download <id1> <id2> --output ./songs/
sunox download <id1> --video --output ./videos/
```

### Subir audio

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
```

## Modelos

| Versión | Codename | Descripción |
|---|---|---|
| **v5.5** | chirp-fenix | Por defecto, mejor calidad actual |
| v5 | chirp-crow | Generación anterior |
| v4.5+ | chirp-bluejay | Capacidades ampliadas |
| v4.5 | chirp-auk | Versión estable |
| v4 | chirp-v4 | Versión antigua |
| v3.5 | chirp-v3-5 | Versión antigua |
| v3 | chirp-v3-0 | Versión antigua |
| v2 | chirp-v2-xxl-alpha | Versión antigua |

Modelos de remaster: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

## Salida amigable para agentes

- Todas las órdenes soportan `--json`.
- Cuando stdout está redirigido, se activa JSON automáticamente.
- El progreso y los errores van a stderr para no contaminar el JSON.
- Las respuestas de error incluyen una acción sugerida.

```bash
sunox clip list | jq '.data[0].title'
sunox agent-info --json
```

Códigos de salida semánticos:

| Código | Significado | Acción sugerida |
|---|---|---|
| 0 | Éxito | Continuar |
| 1 | Error runtime o de red | Reintentar con backoff |
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

Los flujos generate, describe, persona, cover y extend reutilizan `/api/generate/v2-web/` de Suno Web. El body custom create fue recapturado el 30 de junio de 2026: las letras personalizadas se envían en `gpt_description_prompt`, mientras `prompt` queda vacío; con un challenge token resuelto también se envía `token_provider: 1`. `task: "playlist_condition"` también fue capturado, pero pertenece a un flujo inspiration separado que pone letras en `prompt`, por lo que no debe reutilizar las reglas del custom create estándar. Remaster usa `/api/generate/upsample`, y speed adjust usa `/api/clips/adjust-speed/`. Por defecto, `sunox` no envía challenge token; usa `--token <solved>` o `--captcha` solo cuando Suno rechace la petición o cuando quieras forzar el solver. Los bodies de cover, concat y playlist mutation todavía necesitan una captura live.

## Contribuir

1. Crea una rama: `git checkout -b feature/your-idea`
2. Haz los cambios y ejecuta `cargo test`
3. Abre una PR

Son especialmente bienvenidos los tests de integración con `assert_cmd` y el soporte de almacenamiento de secretos mediante OS keychain / Secret Service / CredMan.

## License

MIT, ver [LICENSE](LICENSE).
