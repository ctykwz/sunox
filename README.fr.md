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

## Installation

### Cargo

```bash
cargo install sunox
```

### Binaires précompilés

Téléchargez les binaires macOS, Linux et Windows depuis [GitHub Releases](https://github.com/ctykwz/sunox/releases).

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
| `-c key=value` / `--config key=value` | Remplace temporairement une configuration, par exemple `-c default_model=v5.5 -c output_dir=./songs` ; répétable |
| `-V` / `--version` | Affiche la version |
| `-h` / `--help` | Affiche l'aide de la commande ou sous-commande |

## Commandes humaines

La plupart des usages quotidiens se limitent à ces entrées :

```text
sunox <prompt>                  Générer depuis une description simple
sunox create [prompt]           Générer avec titre, tags, paroles, modèle, persona
sunox download <clip_ids>       Télécharger les morceaux terminés
sunox add <clip_ids> --to <id>  Ajouter des morceaux à une playlist
sunox login                     Configurer l'authentification depuis le navigateur
sunox logout                    Supprimer l'authentification locale
sunox doctor                    Diagnostiquer la configuration et l'auth
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
sunox clip remaster       Remasteriser avec un autre modèle
sunox clip speed          Ajuster la vitesse de lecture
sunox clip stems          Extraire les stems voix et instrumentaux
```

### Parcourir et inspecter

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

### Gérer les ressources

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

### Configuration et auth

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

## Fonctionnalités

### Authentification sans friction

```bash
sunox login
```

`sunox` lit le cookie Clerk depuis Chrome, Arc, Brave, Firefox ou Edge, l'échange contre un JWT, stocke une session locale renouvelable et rafraîchit automatiquement les JWT expirés.

Méthodes d'authentification :

1. `sunox login` : extraction automatique depuis le navigateur, recommandée.
2. `sunox auth --cookie <cookie>` : collage manuel d'un cookie sur serveur headless.
3. `sunox auth --jwt <token>` : JWT direct, généralement valable environ 1 heure.
4. `sunox auth --refresh` : force un nouveau JWT depuis la session Clerk sauvegardée.

### Paramètres de génération

| Paramètre | Rôle | Valeurs |
|---|---|---|
| `--title` | Titre du morceau | jusqu'à 100 caractères |
| `--tags` | Direction de style | par exemple `"pop, synths, upbeat"` |
| `--exclude` | Styles à éviter | par exemple `"metal, heavy, dark"` |
| `--lyrics` / `--lyrics-file` | Paroles personnalisées | sections comme `[Verse]` prises en charge |
| `--prompt` | Prompt du mode description | jusqu'à 500 caractères |
| `--model` | Version du modèle | v5.5, v5, v4.5+, v4.5, v4, v3.5, v3, v2 |
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

### Transformations de clips

```bash
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip remaster <clip_id> --model v5.5
sunox clip speed <clip_id> --multiplier 0.94
sunox clip wait <new_clip_id>
sunox download <new_clip_id> --output ./remastered/
```

### Télécharger et intégrer les paroles

Lors du téléchargement MP3, `sunox` écrit automatiquement :

- **USLT** : paroles simples.
- **SYLT** : paroles synchronisées mot à mot.

```bash
sunox download <id1> <id2> --output ./songs/
sunox download <id1> --video --output ./videos/
```

### Importer de l'audio

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"
```

## Modèles

| Version | Codename | Description |
|---|---|---|
| **v5.5** | chirp-fenix | Par défaut, meilleure qualité actuelle |
| v5 | chirp-crow | Génération précédente |
| v4.5+ | chirp-bluejay | Capacités étendues |
| v4.5 | chirp-auk | Version stable |
| v4 | chirp-v4 | Ancienne version |
| v3.5 | chirp-v3-5 | Ancienne version |
| v3 | chirp-v3-0 | Ancienne version |
| v2 | chirp-v2-xxl-alpha | Ancienne version |

Modèles de remaster : v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

## Sortie adaptée aux agents

- Chaque commande prend en charge `--json`.
- stdout redirigé active automatiquement le JSON.
- Les progrès et erreurs vont sur stderr pour ne pas polluer le JSON.
- Les réponses d'erreur contiennent une action suggérée.

```bash
sunox clip list | jq '.data[0].title'
sunox agent-info --json
```

Codes de sortie sémantiques :

| Code | Signification | Action suggérée |
|---|---|---|
| 0 | Succès | Continuer |
| 1 | Erreur runtime ou réseau | Réessayer avec backoff |
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

Les chemins generate, describe, persona, cover et extend réutilisent `/api/generate/v2-web/` de Suno Web. Le body custom create a été recapturé le 30 juin 2026 : les paroles personnalisées sont envoyées dans `gpt_description_prompt`, tandis que `prompt` reste vide ; avec un challenge token résolu, `token_provider: 1` est aussi envoyé. `task: "playlist_condition"` a également été capturé, mais c'est un flux inspiration séparé qui place les paroles dans `prompt`, donc il ne doit pas reprendre les règles du custom create standard. Le remaster utilise `/api/generate/upsample`, et speed adjust utilise `/api/clips/adjust-speed/`. Par défaut, `sunox` ne soumet pas de challenge token ; utilisez `--token <solved>` ou `--captcha` uniquement quand Suno refuse la requête ou quand vous voulez forcer le solveur. Les bodies cover, concat et playlist mutation nécessitent encore une capture live.

## Contribution

1. Créez une branche : `git checkout -b feature/your-idea`
2. Modifiez le code et lancez `cargo test`
3. Ouvrez une PR

Les tests d'intégration `assert_cmd` et le stockage des secrets via OS keychain / Secret Service / CredMan sont particulièrement bienvenus.

## License

MIT, voir [LICENSE](LICENSE).
