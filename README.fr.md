<div align="center">

# sunox

**Générez de la musique IA depuis le terminal avec les workflows Web de Suno**

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

`sunox` est un binaire Rust unique qui appelle directement les endpoints Web de Suno. Il prend en charge les paroles personnalisées, les tags de style, les voice personas, le contrôle vocal, les curseurs weirdness/style, les covers, les remasters, les changements de vitesse, l'extraction de stems et l'intégration automatique des paroles lors du téléchargement.

**Langues:** [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | Français | [Español](README.es.md)

[Installation](#installation) | [Démarrage rapide](#démarrage-rapide) | [Commandes humaines](#commandes-humaines) | [Commandes agent et avancées](#commandes-agent-et-avancées) | [Fonctionnalités](#fonctionnalités) | [Contribution](#contribution)

</div>

## Pourquoi

L'interface Web de Suno fonctionne bien pour une utilisation manuelle, mais elle n'est pas pensée pour les scripts, les paroles lues depuis un fichier, la génération par lots ou les workflows musicaux pilotés depuis un terminal.

`sunox` corrige cela : authentification automatique depuis le navigateur, paramètres de génération exposés en flags CLI, sortie lisible par un humain ou structurée en JSON, et paroles synchronisées intégrées automatiquement dans les MP3 téléchargés.

Sunox est un projet non officiel, sans affiliation ni approbation de Suno. Il utilise des API Web privées susceptibles de changer sans préavis. Il vous appartient de respecter les conditions de Suno, les limites du compte et les droits applicables aux contenus générés ou téléversés.

## Installation

### Cargo

```bash
cargo install sunox
```

Rust 1.88 ou une version plus récente est requis.

### Binaires précompilés

Téléchargez les binaires macOS, Linux et Windows depuis [GitHub Releases](https://github.com/ctykwz/sunox/releases).
Chaque release fournit `SHA256SUMS` ; `sunox update` vérifie l'archive choisie avant installation.

### Mise à jour intégrée

```bash
sunox update --check    # voir si une nouvelle version existe
sunox update            # installer la dernière release
```

Quand Suno modifie son schéma Web, lancez d'abord `sunox update`. C'est souvent plus rapide que d'attendre une mise à jour via un gestionnaire de paquets.

## Démarrage rapide

```bash
# 1. Connexion, avec extraction automatique depuis Chrome / Arc / Brave / Firefox / Edge
sunox login

# 2. Générer à partir d'une description naturelle
sunox "un morceau chill lo-fi sur un matin pluvieux"

# 3. Générer avec contrôle complet
sunox create \
  --title "Weekend Code" \
  --tags "indie rock, guitar, upbeat" \
  --exclude "metal, heavy" \
  --lyrics-file lyrics.txt \
  --vocal male \
  --weirdness 40 \
  --style-influence 65

# 4. Attendre les clip IDs retournés, puis télécharger l'audio terminé
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs/

# 5. Ajouter un résultat à une playlist
sunox add <clip_id> --to <playlist_id>
```

Pour les agents et scripts, commencez par `sunox agent-info --json`, puis appelez les commandes de ressources avec `--json`.

## Options globales

| Option | Description |
|---|---|
| `--json` | Force une sortie JSON structurée ; activé automatiquement quand stdout est redirigé |
| `--quiet` | Réduit les messages de progression non essentiels |
| `--parallel` | Autorise des écritures Suno concurrentes pour le même compte ; par défaut elles sont sérialisées par compte |
| `-c key=value` / `--config key=value` | Remplace temporairement une configuration, par exemple `-c default_model=v5.5 -c output_dir=./songs` ; répétable |
| `-V` / `--version` | Affiche la version |
| `-h` / `--help` | Affiche l'aide de la commande ou sous-commande |

Les écritures Suno sont sérialisées par compte par défaut. Désactivez ce
comportement de façon persistante avec `sunox config set serial_mutations false`,
pour une invocation avec `-c serial_mutations=false`, ou pour une seule commande
avec `--parallel`.
Les variables d'environnement utilisent le préfixe `SUNOX_*`, par exemple `SUNOX_DEFAULT_MODEL`, `SUNOX_OUTPUT_DIR` et `SUNOX_BROWSER_PATH`.

## Commandes humaines

La plupart des usages quotidiens se limitent à ces entrées :

```text
sunox <prompt>                  Générer depuis une description simple
sunox create [prompt]           Générer avec titre, tags, paroles, modèle, persona
sunox download <clip_ids>       Télécharger les morceaux terminés
sunox add <clip_ids> --to <id>  Ajouter des morceaux à une playlist
sunox login                     Configurer l'authentification depuis le navigateur
sunox logout                    Supprimer l'auth locale et le profil login interactif
sunox doctor                    Diagnostiquer la configuration et l'auth
sunox doctor --network          Diagnostiquer DNS, TCP et HTTPS (`--strict` renvoie une erreur en cas de dégradation)
```

## Commandes agent et avancées

`sunox` garde disponibles les workflows Suno de bas niveau pour les agents de type Codex, l'automatisation et le débogage. Les agents devraient privilégier `--json` et découvrir le contrat courant avec `sunox agent-info --json`.

### Création et transformation

```text
sunox create              Mode description ou paroles personnalisées
sunox lyrics              Générer uniquement des paroles, sans consommer de credits
sunox clip extend         Continuer un clip depuis un timestamp
sunox clip concat         Assembler des clips en chanson complète
sunox clip cover          Créer une cover avec un autre style ou modèle
sunox clip inspire        Générer un nouveau morceau inspiré librement d'un clip
sunox clip remaster       Remasteriser avec un autre modèle
sunox clip speed          Ajuster la vitesse de lecture
sunox clip reverse        Inverser l'audio
sunox clip crop           Garder une section ou retirer une section
sunox clip fade           Ajouter un fondu entrant/sortant
sunox clip stems          Générer des stems depuis un clip existant
```

### Parcourir et inspecter

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

### Gérer les ressources

```text
sunox download <ids>       MP3 CDN par défaut ; --format mp3|m4a|wav|opus est explicite
sunox clip download <ids>  Équivalent avancé/agent de download
sunox clip upload <file>
sunox clip upload-status <upload_id>
sunox clip delete <ids> -y
sunox clip restore <ids>
sunox clip purge <ids> -y       # suppression definitive depuis la corbeille
sunox clip empty-trash -y       # vider la corbeille de façon irréversible
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

### Configuration et auth

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

## Fonctionnalités

Les fonctionnalités Studio sont hors du périmètre de ce CLI.

### Authentification sans friction

```bash
sunox login
```

`sunox login` essaie d'abord de lire le cookie Clerk depuis Chrome, Arc, Brave, Firefox ou Edge. Sous Windows, il évite les bases Chromium actives afin de ne pas déclencher le déverrouillage App-Bound ni fermer le navigateur, mais lit Firefox par un accès SQLite non destructif en lecture seule. Sans session réutilisable, le profil interactif dédié exige un navigateur de la famille Chromium. Sunox associe la session au profil et au canal exacts, puis interroge le même binaire pour obtenir le user-agent, les langues et les Client Hints sans fenêtre supplémentaire ni accès à Suno. Les nouvelles valeurs priment, les anciennes sont conservées champ par champ si la sonde échoue, et les constantes intégrées ne servent qu'en dernier recours pour Clerk et l'API Suno.

Les identifiants sont stockés dans un fichier JSON local, pas dans le trousseau du système. Sous Unix, le fichier est créé avec le mode `0600` ; sous Windows, Sunox dépend de l'ACL utilisateur du dossier de configuration. Les valeurs `--cookie` et `--jwt` peuvent apparaître dans l'historique du shell et la liste des processus : préférez `sunox login` ou `--cookie-stdin` / `--jwt-stdin`, et ne placez jamais d'identifiants dans des logs, prompts, fichiers de projet ou commits.

Méthodes d'authentification :

1. `sunox login` : extraction automatique depuis le navigateur, avec fallback Chrome/Edge interactif, recommandée.
2. `printf '%s' "$SUNOX_COOKIE_INPUT" | sunox auth --cookie-stdin` : lecture du cookie depuis stdin.
3. `printf '%s' "$SUNOX_JWT_INPUT" | sunox auth --jwt-stdin` : lecture du JWT depuis stdin.
4. `sunox auth --refresh` : force un nouveau JWT depuis la session Clerk sauvegardée.

`sunox logout` supprime les identifiants locaux, le profil de login interactif et l'ancien profil captcha.

### Paramètres de génération

| Paramètre | Rôle | Valeurs |
|---|---|---|
| `--title` | Titre du morceau | jusqu'à 100 caractères |
| `--tags` | Direction de style | Limite du modèle/compte ; voir `sunox models --json` |
| `--enhance-tags` | Améliorer les tags via le flux tag upsample de Suno Web avant l'envoi | opt-in explicite |
| `--exclude` | Styles à éviter | Limite du modèle/compte ; voir `sunox models --json` |
| `--lyrics` / `--lyrics-file` | Paroles personnalisées | `max_lengths.gpt_description_prompt` |
| `--prompt` | Prompt du mode description | `max_lengths.prompt` |
| `--model` | Version du modèle | v5.5, v5, v4.5+, v4.5-all, v4.5, v4, v3.5, v3, v2 |
| `--vocal` | Genre vocal | male, female |
| `--persona` | ID de voice persona | UUID de la voix dans Suno |
| `--weirdness` | Niveau expérimental | 0-100 |
| `--style-influence` | Fidélité au style | 0-100 |
| `--instrumental` | Instrumental sans voix | flag |

### Voice personas

```bash
sunox persona list
sunox persona info <persona_id>
sunox persona create <clip_id> --name "My Voice" --description "Warm lead vocal"
sunox create --persona <persona_id> --title "My Song" --tags "pop" --lyrics "[Verse]\nHello world"
```

Vous pouvez aussi publier, dépublier, aimer, supprimer, restaurer ou purger une persona :

```bash
sunox persona publish <persona_id>        # uniquement si vous voulez la rendre publique
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id>
sunox persona purge <persona_id> -y       # suppression définitive
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

### Transformations de clips

```bash
# Ces commandes peuvent renvoyer un clip submitted/processing ; attendez avant toute action suivante
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip inspire <clip_id> --title "New Song" --tags "garage pop" --lyrics-file lyrics.txt
sunox clip remaster <clip_id> --model v5.5 --variation subtle # subtle, normal ou high
sunox clip speed <clip_id> --multiplier 0.94
sunox clip reverse <clip_id>
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/

# crop/fade attendent déjà que le clip résultat soit complete ; aucun second wait n'est requis
sunox clip crop <clip_id> --start 12.5 --end 74.0
sunox clip crop <clip_id> --start 30.0 --end 45.0 --remove-section
sunox clip fade <clip_id> --in 2.0 --out 78.5
```

### Télécharger et intégrer les paroles

Lors du téléchargement MP3, `sunox` écrit automatiquement :

- **USLT** : paroles simples.
- **SYLT** : paroles synchronisées mot à mot.

```bash
sunox download <id1> <id2> --output ./songs/

# N'utilisez --force que pour remplacer explicitement un fichier existant
sunox download <id1> --output ./songs/ --force
sunox download <id1> --format wav --output ./songs/
sunox download <id1> --video --output ./videos/
```

Les fichiers suivent le format `title-slug-clipid8.<ext>`. Les répertoires de sortie sont créés automatiquement et les fichiers existants sont conservés sauf avec `--force`.

### Importer de l'audio

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
sunox clip upload-status <upload_id> --json  # lecture seule, sans rejouer la mutation
```

## Modèles

| Version | Codename | Description |
|---|---|---|
| auto | réponse du compte | Valeur CLI par défaut ; choisit le modèle utilisable par défaut du compte |
| v5.5 | chirp-fenix | Génération la plus récente ; fallback uniquement si billing est indisponible |
| v5 | chirp-crow | Génération précédente |
| v4.5+ | chirp-bluejay | Capacités étendues |
| v4.5-all | chirp-auk-turbo | Option gratuite lorsqu'elle est proposée au compte |
| v4.5 | chirp-auk | Version stable |
| v4 | chirp-v4 | Ancienne version |
| v3.5 | chirp-v3-5 | Ancienne version |
| v3 | chirp-v3-0 | Ancienne version |
| v2 | chirp-v2-xxl-alpha | Ancienne version |

Modèles de remaster : v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

La disponibilité, le modèle par défaut du compte et les limites dépendent du compte. `default_model=auto` choisit directement le modèle utilisable par défaut depuis `/api/billing/info/`; `sunox models --json` expose les mêmes données de compte pour inspection. Un modèle explicite est validé avec `can_use` et `max_lengths` lorsque billing est disponible ; v5.5 ne sert de fallback que si cette lecture échoue.

## Sortie adaptée aux agents

- Chaque commande prend en charge `--json`.
- stdout redirigé active automatiquement le JSON.
- Les progrès et erreurs vont sur stderr pour ne pas polluer le JSON.
- Les écritures Suno sont sérialisées par compte par défaut ; n'utilisez pas `sunox config set serial_mutations false`, `-c serial_mutations=false` ou `--parallel` sauf si l'utilisateur autorise explicitement des écritures concurrentes sur le même compte.
- Pour une inspection audio courante, utilisez le média existant du clip : `sunox clip info <id> --json` expose `audio_url` ainsi que `attribution`, `comments`, `direct_children_count` et `similar_clips`; si une lecture complémentaire échoue sans erreur d'authentification ni de limite de débit, le clip de base est quand même renvoyé avec `supplemental_errors`. Les erreurs d'authentification et de limite de débit interrompent toujours normalement. Par défaut, `sunox clip download` télécharge le MP3 CDN de `audio_url` et y intègre les paroles ; `--format mp3|m4a|wav|opus` demande explicitement le format officiel Suno, et `--video` utilise `clip.video_url` lorsqu'il existe. `sunox clip stems` est une extraction de stems basée sur la génération, distincte de l'export Pro Get Stems de Suno Web. Les agents ne doivent demander un format explicite, des stems ou la vidéo que lorsque l'utilisateur le demande. `--quiet` supprime la progression du téléchargement et les sorties d'état ordinaires. Si un téléchargement par lot renvoie `partial_download`, inspectez `error.details.succeeded`, `error.details.failed` et `error.details.not_attempted_clip_ids`, puis ne réessayez que les ID nécessaires. Si `playlist remove` ou une publication/réaction sur plusieurs clips renvoie `partial_mutation`, inspectez `error.details.succeeded_clip_ids`, `error.details.failed` et `error.details.not_attempted_clip_ids` avant de réessayer.
- La création/mise à jour d'une playlist, l'upload d'une image locale, la pochette d'un clip et l'upload audio sont des workflows à plusieurs étapes. Un échec renvoie `partial_mutation` avec les identifiants, `completed_steps`, `failed.step/code/message` et `recovery`. Ne suivez la commande structurée que si `recovery.resumable=true` et ne rejouez jamais une mutation marquée false. L'audio est envoyé en streaming et les métadonnées sont relues jusqu'à ce que les champs demandés soient visibles. `clip upload-status` est strictement en lecture seule.
- Sans demande explicite de l'utilisateur, ne publiez pas de ressource, ne forcez pas `--captcha`, n'affichez pas de secrets d'authentification et n'exécutez pas de commandes destructrices ; ces commandes exigent `-y/--yes`.
- Les réponses d'erreur contiennent une action suggérée.

```bash
sunox clip list | jq '.data.clips[0].title'
sunox clip list --liked --public --sort popular --json
sunox agent-info --json
```

Codes de sortie sémantiques :

| Code | Signification | Action suggérée |
|---|---|---|
| 0 | Succès | Continuer |
| 1 | Erreur runtime, endpoint Web, mutation partielle ou téléchargement partiel | Inspecter `error.code` et `error.details` avant de réessayer |
| 2 | Erreur de configuration | Corriger la config, ne pas réessayer à l'aveugle |
| 3 | Erreur d'authentification | Lancer `sunox login` |
| 4 | Limite de débit | Attendre 30-60 secondes |
| 5 | Ressource introuvable | Vérifier l'ID |

## Installer comme skill pour agent de code

```bash
# Codex / Trae CLI
sunox install-skill

# Claude Code
sunox install-skill --target claude

# Cursor
sunox install-skill --target cursor
```

## Notes d'implémentation

Les chemins generate, describe, persona, cover et extend réutilisent `/api/generate/v2-web/` de Suno Web. Le body custom create a été recapturé le 30 juin 2026 : les paroles personnalisées sont envoyées dans `gpt_description_prompt`, tandis que `prompt` reste vide ; avec un challenge token résolu, le `token_provider` correspondant à la version renvoyée par le preflight Web est envoyé. Sunox renseigne `metadata.user_tier` depuis le `plan.id` de `/api/billing/info/` pour le compte courant quand c'est disponible, sinon il retombe sur la valeur vide compatible Web. Avec `--enhance-tags`, Sunox appelle d'abord `/api/prompts/upsample`, puis place les tags et le `request_id` retournés dans `metadata.last_tags_generation` et marque `override_fields=["tags"]`; le champ `personalization_enabled` suit la forme du submit Web capturé. Sans ce flag, `metadata.last_tags_generation` n'est pas envoyé. Instrumental create utilise aussi custom mode : avec `sunox create --instrumental <prompt>`, le prompt est intégré aux style tags et le champ `prompt` soumis reste vide, comme dans la requête Web recapturée dans `15suno-labs-nostudio-20260630.har`. `task: "playlist_condition"` a également été capturé, mais c'est un flux inspiration séparé qui place les paroles dans `prompt`, donc il ne doit pas reprendre les règles du custom create standard. Extend lit le clip source avant submit ; si `GET /api/feed/?ids` omet les métadonnées de style source, Sunox cherche le titre source via feed/v3 et fusionne seulement les métadonnées du clip id exact. `title` prend le titre source sauf si `--title` est fourni ; `tags`, `negative_tags` et `metadata.make_instrumental` sont hérités quand disponibles. Utilisez `--tags`, `--exclude`, `--instrumental` ou `--no-instrumental` pour remplacer ces valeurs. `clip list` utilise `POST /api/feed/v3` et expose des filtres de requête comme `--liked`, `--public`, `--upload`, `--cover`, `--extend` et `--sort popular`; ce n'est pas un workflow de library sync. Le remaster utilise `/api/generate/upsample`, et speed adjust utilise `/api/clips/adjust-speed/`. Sunox exécute `/api/c/check` avant la génération et, si une session Clerk peut être rafraîchie, renouvelle le JWT une fois puis relance le preflight. Si le challenge reste requis, il exécute silencieusement hCaptcha/provider 1 ou Cloudflare Turnstile/provider 2 selon `captcha_version`, en privilégiant les cookies vérifiés du compte et la source de navigateur enregistrée. `--token` fournit un token externe, `--captcha` force la vérification et `--no-captcha` désactive la vérification automatique. Les bodies cover generation et concat edit nécessitent encore une nouvelle capture live. Les mutations de playlists sont implémentées à partir d'indices bundle/live et de tests de contrat endpoint ; `playlist remove` envoie un clip par requête, car les gros lots peuvent retourner Suno 500.

`sunox clip inspire` implémente le `task=playlist_condition` capturé en production : une seule source, tag upsample réel et paroles dans `prompt`. Les variantes multi-source et instrumentales non capturées ne sont pas exposées. Les variables d'environnement publiques utilisent le préfixe `SUNOX_*`.

## Contribution

1. Créez une branche : `git checkout -b feature/your-idea`
2. Modifiez le code et lancez `cargo test`
3. Ouvrez une PR

Les tests d'intégration `assert_cmd` et le stockage des secrets via OS keychain / Secret Service / CredMan sont particulièrement bienvenus.

## License

MIT, voir [LICENSE](LICENSE).
