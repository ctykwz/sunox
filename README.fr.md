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
Suno. Si aucune vérification n'est demandée, la requête part directement et aucun navigateur n'est
lancé. Si Suno exige un challenge, Sunox demande d'abord à l'extension facultative Browser Bridge
d'exécuter le widget invisible dans le profil Chrome habituel. Au repos, l'extension ne conserve que
son listener local. Lorsqu'une vérification est nécessaire, elle utilise le document offscreen
invisible de Chrome et y place un iframe `suno.com` unique lié à un nonce. L'iframe conserve un
viewport normal pour le fournisseur, mais Chrome ne crée ni onglet, ni popup, ni fenêtre réduite,
ni processus de navigateur séparé. Seul l'iframe de premier niveau appartenant à l'extension peut
se connecter. Il utilise un stockage temporaire isolé et ne peut donc lire ni les cookies Suno ni
les données persistantes de l'utilisateur. À `document_start`, le Bridge arrête la réponse du site
et la remplace par un document minimal qui n'autorise que le fournisseur de vérification actif.
Le nonce réseau, le nonce du document et les en-têtes finaux doivent tous correspondre. Toute
navigation, mise en cache, recharge, déconnexion ou identité inattendue supprime l'iframe et fait
échouer la requête de façon sûre. L'iframe est aussi supprimé après le jeton ou l'erreur finale,
sans recours à un contexte visible ou authentifié. Ce comportement est pris en charge sous macOS
comme sous Windows.

Si le Bridge ne répond pas, le mode `auto` par défaut ne se replie sur un navigateur de la famille
Chromium installé que lorsqu'aucune installation du Bridge n'a été enregistrée. Une fois le Bridge installé,
`auto` échoue de façon sûre au lieu de lancer un processus de navigateur séparé. Utilisez
explicitement `challenge_browser=isolated` lorsque ce recours indépendant est acceptable.

### Installer Browser Bridge sur macOS ou Windows

Browser Bridge est fourni avec le binaire Sunox : aucun ZIP séparé ni installation depuis le Chrome
Web Store n'est nécessaire. La procédure est identique sur macOS et Windows :

1. Extrayez l'extension fournie et notez le répertoire affiché par la commande :

   ```bash
   sunox install-browser-extension
   ```

2. Dans le même profil Chrome que celui où vous utilisez Suno, ouvrez `chrome://extensions`.
3. Activez le **mode développeur**, choisissez **Charger l'extension non empaquetée** et
   sélectionnez exactement le répertoire affiché par Sunox. Sous macOS, appuyez sur
   `Shift+Command+G` dans le sélecteur de dossier et collez le chemin, car `~/Library` est masqué
   par défaut. Sous Windows, collez le chemin affiché dans la barre d'adresse du sélecteur.
4. Laissez l'extension activée. Aucun onglet Suno ne doit rester ouvert.

Vérifiez le transport du Bridge sans créer de morceau, exécuter de challenge ni consommer de crédits :

```bash
sunox doctor --browser-bridge
```

L'extension reste installée après le redémarrage du navigateur. Son manifeste utilise l'identité
indépendante du runtime Bridge : une version qui ne modifie que le CLI ne change donc pas le paquet
extrait et n'impose aucun rechargement de Chrome. Si une mise à jour de Sunox modifie réellement le
Bridge, actualisez ses fichiers :

```bash
sunox install-browser-extension --force
```

La commande conserve l'état d'activation jusqu'à ce que Chrome authentifie exactement le runtime et
l'appairage. La première extraction renvoie `status=installed`, `reload_required=null`,
`runtime_ack_pending=true`, `pending_origin=load_unpacked` et
`activation_required=load_unpacked` : terminez **Charger l'extension non empaquetée**, puis exécutez
`sunox doctor --browser-bridge`. La seule présence des fichiers ne signifie pas que le Bridge est
prêt.

Lorsqu'une installation déjà authentifiée change, la commande renvoie `reload_required=true`,
`runtime_ack_pending=true` et `activation_required=reload` : cliquez exactement une fois sur
**Actualiser**, puis exécutez doctor. Si l'état Chrome d'une installation jamais authentifiée ou
restaurée est incertain, `activation_required=ensure_loaded` ; `activation_options` contient des
alternatives conditionnelles telles que `load_unpacked_if_missing` ou
`enable_and_reload_if_present`. Ce sont des branches mutuellement exclusives, pas des étapes
successives. Un paquet déjà à jour est sondé activement : une authentification exacte efface le
marqueur et renvoie `reload_required=false`, `runtime_ack_pending=false` et aucune activation. Si
l'état reste inconnu, la commande renvoie `reload_required=null` et
`runtime_ack_pending=true` ; suivez l'unique décision `activation_required` au lieu de cliquer en
boucle sur Actualiser.

Doctor distingue un secret d'appairage absent ou corrompu mais réparable et prescrit une seule
réparation gérée avec `install-browser-extension --force`. Les entrées dangereuses ou inaccessibles,
notamment les liens symboliques, les données non UTF-8 et les chemins illisibles, échouent de manière
fermée sans promettre que force ou Actualiser puissent les réparer. Une mise à jour du CLI seul, ou
le seul redémarrage de l'ordinateur ou de Chrome, n'impose jamais à lui seul de réinstaller ni de
recharger le Bridge. Il n'est pas nécessaire de recharger une page Suno. La commande choisit le
répertoire d'application propre à chaque utilisateur sur macOS comme sur Windows ; ne déplacez pas
et ne supprimez pas ce répertoire tant que Chrome utilise l'extension non empaquetée.

```text
--captcha          Effectuer la vérification même si le contrôle initial ne la demande pas
--no-captcha       Désactiver la résolution automatique dans le navigateur
--token <token>    Utiliser un jeton de challenge obtenu ailleurs
```

Réglez `challenge_browser` sur `auto` (par défaut), `existing` (exige le Bridge et ne lance jamais
de processus de navigateur séparé) ou `isolated` (utilise toujours le navigateur temporaire). Vous
pouvez le remplacer pour une commande avec `-c challenge_browser=existing`. Le nom `existing` est
conservé pour la compatibilité de configuration : il signifie désormais « utiliser le Bridge
installé dans le profil Chrome existant ». Le Bridge crée et supprime automatiquement un iframe
offscreen lié à un nonce ; il n'ouvre ni onglet ni fenêtre. Un Bridge déjà configuré mais absent ou obsolète est signalé
comme une erreur au lieu d'ouvrir un autre navigateur ou un contexte visible. En mode `auto`, Sunox peut ouvrir le secours isolé uniquement lorsqu'aucune installation du
Bridge n'a été enregistrée. Un Bridge installé mais désactivé, obsolète, inaccessible ou privé de
son secret d'appairage échoue de façon
sûre ; utilisez explicitement `isolated` pour autoriser un processus de navigateur séparé.

Pour les exécutions sans surveillance qui ne doivent ni ajouter un onglet Suno à la fenêtre active
ni lancer un autre processus de navigateur, installez Browser Bridge et omettez `--no-captcha`.
`auto` comme `challenge_browser=existing` échouent alors de façon sûre si le Bridge est indisponible ;
`existing` exige en outre le Bridge même si aucun appairage n'est configuré. Si le Bridge n'est pas
installé ou si son état est inconnu, conservez `--no-captcha` : un challenge requis s'arrêtera avant
l'envoi. Sans Bridge configuré, omettre `--no-captcha` en mode `auto` par défaut autorise encore le
recours au navigateur isolé.

Installer le Bridge constitue une autorisation permanente pour que Sunox exécute des challenges
dans le contexte éphémère qu'il gère automatiquement ; aucune autorisation distincte n'est
nécessaire à chaque génération. Des demandes telles que « ne pas laisser d'onglet Suno ouvert »,
« pas de nouveau navigateur » ou « pas de captcha visible » autorisent le Bridge installé et ne
signifient pas `--no-captcha` ; `challenge_browser=existing` reste le réglage explicite réservé au
Bridge. Ne conservez `--no-captcha` malgré un Bridge installé que si tous les mécanismes de
challenge, y compris le Bridge, sont explicitement interdits ou si cette option exacte est demandée.

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
