# File Cleaner

Dépôt GitHub : **https://github.com/Les-Doubles-A/FileCleaner**

Application de bureau **100 % locale** pour analyser, comprendre et nettoyer un
disque en toute sécurité. Écrite entièrement en Rust : un seul exécutable, aucune
dépendance à installer, aucune donnée envoyée sur internet.

## Fonctionnalités

**Comprendre**
- Tableau de bord : score de propreté, gains possibles, répartition par type, évolution entre deux analyses
- Carte du disque : treemap interactive avec exploration par dossier
- Recherche instantanée avec filtres combinables
- Inspecteur : aperçu d'image ou de texte, propriétés, actions rapides

**Nettoyer**
- Doublons par contenu (taille, empreinte partielle, puis BLAKE3 complet) avec cache persistant et détection des liens durs
- Sélection automatique selon une stratégie (plus ancien, plus récent, chemin, dossier à garder)
- Images similaires par hachage perceptuel, même redimensionnées
- Fichiers inutiles classés par risque, avec règles personnalisées et nettoyage express
- Gros fichiers, fichiers anciens, extensions rares, dossiers réellement vides
- Projets de dev : `node_modules`, `target`, `.venv`, `__pycache__`... (22 types, marqueur de projet vérifié)
- Nettoyage Windows : temporaires, rapports de plantage, caches navigateurs, npm, pip, Discord, Corbeille

**Organiser**
- Classement par gabarit (`{category}/{year}`) avec aperçu, projets protégés
- Renommage par lots : modèles, rechercher/remplacer, regex, casse, accents, aperçu ligne à ligne

**Partager**
- Rapports HTML, PDF, CSV et JSON
- Outil en ligne de commande `fc`, en lecture seule

**Confort**
- Palette de commandes (Ctrl K), glisser-déposer, thème clair/sombre/système, français et anglais
- Sélection globale conservée d'une page à l'autre, analyse de plusieurs dossiers à la fois
- Entrée « Analyser avec File Cleaner » dans le menu de l'Explorateur (optionnelle)

## Sécurité

- Rien n'est supprimé sans passer par l'écran de revue, qui montre chaque élément
- Trois destinations : Corbeille, quarantaine locale, ou suppression définitive (confirmation renforcée)
- Tout est annulable depuis l'historique, Corbeille comprise, sauf la suppression définitive et le vidage de la Corbeille
- Un fichier modifié depuis l'analyse n'est pas touché, et aucune opération n'écrase un fichier existant
- Dossiers système, racines de lecteurs et dossiers personnels essentiels sont refusés
- Liens symboliques et jonctions ne sont jamais suivis, les fichiers cloud non téléchargés ne sont jamais lus
- Une sélection qui ne laisserait aucune copie d'un doublon est signalée

## Recherche

```
rapport -brouillon ext:pdf,docx size:>10mb age:>365 in:projets cat:images
modified:>2024-01-31 size:1mb..5mb re:^IMG_\d+ is:hidden "nom avec espaces"
```

## Raccourcis

| Touche | Action |
| --- | --- |
| Ctrl K | Palette de commandes |
| Ctrl O | Analyser un dossier |
| Ctrl F | Rechercher |
| F5 | Relancer l'analyse |
| Ctrl Z | Annuler la dernière action |
| Suppr | Nettoyer la sélection |
| Flèches, Espace, Ctrl A | Naviguer et cocher dans les listes |
| Maj + clic, Ctrl + clic | Sélection par plage, sélection simple |

## Compilation

Prérequis : Rust 1.95 ou plus récent et les outils de build MSVC.

```powershell
cargo build --release
```

Les exécutables sont dans `target/release/` : `FileCleaner.exe` et `fc.exe`.

```powershell
cargo test --workspace
```

## Ligne de commande

```powershell
fc scan D:\Photos
fc dupes D:\Photos --json
fc junk .
fc big . --min-mb 500
fc dev C:\Projets
fc search . "ext:pdf size:>10mb"
fc export . -o rapport.html -f html
```

## Architecture

```
crates/core   moteur sans interface : scan, doublons, analyses, actions, historique, exports
crates/app    interface egui/eframe
crates/cli    commande fc
assets        icône et police d'icônes Phosphor (MIT)
legacy_python ancienne version Python, conservée pour référence
```

Les données sont dans `%APPDATA%\FileCleaner` (réglages) et `%LOCALAPPDATA%\FileCleaner`
(historique, cache, journaux). `FILECLEANER_HOME` permet de les rediriger.

## Vérification visuelle

La fonctionnalité `shots` parcourt toutes les pages, enregistre une capture de chacune,
puis enchaîne un vrai nettoyage et son annulation. À n'utiliser que sur un dossier jetable.

```powershell
$env:FC_SHOTS = "captures"; $env:FC_ROOT = "dossier-jetable"
cargo run -p filecleaner-app --features shots
```

<!-- installeur-release -->
## Installation

Téléchargez **`FileCleaner-Setup-2.0.0.exe`** depuis la [dernière release](https://github.com/Les-Doubles-A/FileCleaner/releases/latest), puis lancez-le.

L'installation se fait pour l'utilisateur courant, sans droits administrateur (par défaut dans `%LOCALAPPDATA%\Programs\FileCleaner`), avec un raccourci dans le menu Démarrer et une désinstallation depuis les Paramètres Windows.

L'installeur n'est pas signé : si Windows SmartScreen l'affiche, cliquez sur « Informations complémentaires » puis « Exécuter quand même ».

### Recréer l'installeur

Compiler l'application (`cargo build --release --workspace`), puis lancer [Inno Setup 6](https://jrsoftware.org/isinfo.php) : `iscc installer\FileCleaner.iss`. Le setup est créé dans `installer\Output\`.
