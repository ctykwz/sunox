# sunox

`sunox` est un outil non officiel en ligne de commande pour utiliser Suno depuis un terminal.
Écrit en Rust et distribué sous forme d'un seul binaire, il couvre la création de morceaux, les
téléchargements, les playlists, les personas vocales, les reprises, le remastering, les retouches
audio et les imports.

[![crates.io](https://img.shields.io/crates/v/sunox)](https://crates.io/crates/sunox)
[![CI](https://github.com/ctykwz/sunox/actions/workflows/ci.yml/badge.svg)](https://github.com/ctykwz/sunox/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md) · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · Français ·
[Español](README.es.md)

> [!WARNING]
> Sunox n'est ni affilié à Suno ni approuvé par Suno. Il s'appuie sur des API Web privées qui
> peuvent changer sans préavis. Il vous appartient de respecter les conditions de Suno, les
> limites de votre compte et les droits liés aux contenus générés ou importés.

## Ce que Sunox sait faire

- Créer un morceau à partir d'une description, de paroles, de styles, d'une persona ou d'une
  consigne instrumentale.
- Attendre la fin d'une génération puis télécharger le résultat en MP3, M4A, WAV, Opus ou vidéo.
- Parcourir, rechercher, modifier, publier, supprimer et restaurer des morceaux.
- Créer une reprise, prolonger, assembler, remasteriser, inverser, découper, fondre, changer la
  vitesse ou générer des pistes séparées.
- Gérer les playlists et les personas vocales, importer un fichier audio ou une pochette.
- Produire un affichage lisible dans le terminal ou du JSON stable pour les scripts et agents.

Les fonctions de Suno Studio ne font pas partie du projet.

## Installation

Avec Rust 1.88 ou une version plus récente :

```bash
cargo install sunox
```

Des binaires prêts à l'emploi pour macOS, Linux et Windows sont également disponibles dans les
[GitHub Releases](https://github.com/ctykwz/sunox/releases). Ils ne sont pas signés avec un
certificat commercial Apple ou Windows ; le système peut donc afficher son avertissement habituel.
Chaque version contient un fichier `SHA256SUMS`, vérifié automatiquement par `sunox update`.

## Connexion

Connectez-vous d'abord à suno.com dans votre navigateur, puis lancez :

```bash
sunox login
```

Sunox cherche une session réutilisable dans Chrome, Edge, Brave, Arc, Chromium ou Firefox. S'il
n'en trouve pas, il ouvre un profil de navigateur séparé afin que vous puissiez vous connecter.

Les identifiants sont conservés dans le répertoire de configuration local de Sunox. Évitez de
passer un cookie ou un JWT directement dans la ligne de commande : ils peuvent apparaître dans
l'historique du shell ou dans la liste des processus. Sur une machine sans interface graphique,
utilisez `--cookie-stdin` ou `--jwt-stdin`.

```bash
sunox doctor
sunox credits
```

## Créer puis télécharger un morceau

Une courte description suffit pour commencer :

```bash
sunox "ambient électronique chaleureux, pulsation lente et synthés doux"
```

Pour fournir des paroles et régler la génération :

```bash
sunox create \
  --title "Night Drive" \
  --tags "dream pop, synth, female vocal" \
  --exclude "metal, aggressive" \
  --lyrics-file lyrics.txt \
  --weirdness 35 \
  --style-influence 70
```

### Modes instrumentaux

Choisissez un seul mode. `--instrumental` ne peut pas être combiné avec `--lyrics` ou
`--lyrics-file` :

- Pour un instrumental sans paroles et sans structure interne imposée, utilisez uniquement
  `--instrumental`.
- Pour contrôler les sections, le rythme, les points de montage ou l'arrangement, omettez
  `--instrumental` et utilisez un fichier dont la première ligne est `[Instrumental]`. Toutes les
  autres lignes non vides doivent rester entre crochets, sans texte susceptible d'être chanté.

Après la génération, exécutez `sunox clip timed-lyrics <clip_id> --json`. Écartez la version si une
seule entrée contient un mot aligné non vide avec `success=true`.

Une génération renvoie normalement deux identifiants de clip. Attendez leur achèvement avant de
télécharger les versions qui vous intéressent :

```bash
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs
```

Sans option de format, Sunox récupère le MP3 déjà disponible sur le CDN et y écrit les paroles
simples et synchronisées lorsqu'elles existent. Utilisez `--format mp3|m4a|wav|opus` uniquement
pour demander une conversion à Suno, ou `--video` pour une vidéo disponible.

## Commandes courantes

```text
sunox <description>                Créer un morceau à partir d'une description
sunox create [description]         Créer avec tous les réglages
sunox lyrics                       Générer uniquement des paroles

sunox clip list                    Lister ses morceaux
sunox clip search <recherche>      Rechercher un morceau
sunox clip info <id>               Afficher les détails
sunox clip wait <ids>              Attendre la fin d'une génération
sunox download <ids>               Télécharger les morceaux terminés

sunox clip cover <id>              Créer une reprise
sunox clip extend <id>             Prolonger un morceau
sunox clip concat <ids>            Assembler plusieurs clips
sunox clip remaster <id>           Remasteriser
sunox clip speed <id>              Changer la vitesse
sunox clip reverse <id>            Inverser l'audio
sunox clip crop <id>               Conserver ou retirer un passage
sunox clip fade <id>               Ajouter un fondu
sunox clip stems <id>              Générer des pistes séparées

sunox playlist list                Lister les playlists
sunox playlist create              Créer une playlist
sunox add <clip_ids> --to <id>     Ajouter des morceaux à une playlist

sunox persona list                 Lister les personas vocales
sunox persona create <clip_id>     Créer une persona à partir d'un morceau

sunox clip upload <fichier>        Importer un fichier audio
sunox models                       Afficher les modèles disponibles
sunox doctor --network             Tester DNS, TCP et HTTPS
sunox update                       Installer la dernière version GitHub
```

Consultez `sunox --help` ou `sunox <commande> --help` pour toutes les options.

## Vérification avant génération

Avant chaque requête de génération, Sunox effectue le même contrôle que l'application Web de
Suno. Si aucune vérification n'est demandée, la requête part directement et aucun navigateur
n'est lancé. Si Suno exige un challenge, Sunox demande d'abord à l'extension Browser Bridge
d'exécuter le widget invisible dans un onglet `suno.com` existant. Sans onglet appairé, le mode
`auto` utilise le navigateur Chromium compatible, puis supprime le profil temporaire.

Browser Bridge est intégré au binaire Sunox et prend en charge macOS et Windows ; aucun
téléchargement séparé ni passage par le Chrome Web Store n'est nécessaire. Exécutez
`sunox install-browser-extension`, copiez le chemin affiché, ouvrez `chrome://extensions` dans le
même profil Chrome que celui utilisé pour Suno, activez le mode développeur, puis choisissez
**Charger l'extension non empaquetée**. Sélectionnez exactement ce dossier et rechargez un onglet
`suno.com` authentifié. Sous macOS, appuyez sur `Shift+Command+G` dans le sélecteur de dossier et
collez le chemin, car `~/Library` est masqué par défaut. Sous Windows, collez le chemin dans la
barre d'adresse du sélecteur.

Après une mise à jour de Sunox, exécutez `sunox install-browser-extension --force`, cliquez sur
**Actualiser** dans la carte de l'extension, puis rechargez l'onglet Suno. Ne déplacez pas et ne
supprimez pas le dossier affiché tant que Chrome utilise l'extension.

```text
--captcha          Effectuer la vérification même si le contrôle initial ne la demande pas
--no-captcha       Désactiver la résolution automatique dans le navigateur
--token <token>    Utiliser un jeton de challenge obtenu ailleurs
```

`challenge_browser` accepte `auto`, `existing` (aucune nouvelle fenêtre) ou `isolated`. Le mode
`existing` renvoie une erreur si le Bridge ne répond pas ; `auto` peut ouvrir le navigateur isolé
de secours.

Pour une exécution sans surveillance qui ne doit jamais ouvrir une nouvelle fenêtre, prenez
l'installation confirmée de Browser Bridge comme critère : s'il est installé, retirez
`--no-captcha` et utilisez `-c challenge_browser=existing`. Ce mode vérifie lui-même la connexion
d'un onglet Suno authentifié et actualisé, et échoue sans ouvrir un autre navigateur si nécessaire.
S'il n'est pas installé ou si son installation est inconnue, conservez `--no-captcha`.

## JSON et automatisation

Toutes les commandes acceptent `--json`. La sortie devient aussi automatiquement du JSON quand
elle est redirigée :

```bash
sunox clip list --json
sunox clip list | jq '.data.clips[0].title'
sunox agent-info --json
```

Les erreurs ont des codes stables et des statuts de sortie non nuls. Lorsqu'une opération en lot
échoue en partie, la réponse distingue les éléments terminés, échoués et non exécutés afin de ne
relancer que ce qui est nécessaire.

Le paquet fournit également un Skill d'utilisation pour les agents de développement :

```bash
sunox install-skill                 # Codex
sunox install-skill --target claude
sunox install-skill --target cursor
```

## Configuration et précautions

```bash
sunox config show
sunox config set output_dir ./songs
sunox config set default_model auto
```

`-c key=value` ne modifie que l'exécution courante. Les variables d'environnement portent le
préfixe `SUNOX_*`.

Par défaut, les écritures d'un même compte sont exécutées l'une après l'autre pour éviter les
conflits. `--parallel` désactive cette protection pour une commande ; ne l'utilisez que si ces
écritures simultanées sont voulues.

Certaines commandes consomment des crédits ou modifient des ressources distantes. Les nouveaux
morceaux, playlists et personas restent privés tant qu'une commande ne demande pas explicitement
leur publication. Les opérations irréversibles exigent `-y` ou `--yes`.

## Développement

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Créez une branche à partir de `main`, puis ouvrez une Pull Request.

## Licence

[MIT](LICENSE)
