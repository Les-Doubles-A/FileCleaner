# File Cleaner

Application de bureau **100 % locale** pour analyser, classer et nettoyer un
dossier en toute securite : aucune donnee n'est jamais envoyee sur internet,
aucune suppression n'est jamais definitive et automatique.

## Fonctionnalites

- Selection d'un dossier par glisser-deposer ou via un explorateur de fichiers
- Scan recursif multithread (nom, extension, taille, dates, hash SHA256, type MIME)
- Classement automatique par categorie (Images, Videos, Audio, Documents, PDF,
  Archives, Code, Applications, Bases de donnees, Polices, E-books, Modeles 3D, Divers)
- Renommage intelligent avec previsualisation editable avant application
- Detection de doublons (par contenu/hash ou par nom) avec strategies de conservation
- Detection des fichiers inutiles (temporaires, cache, logs, miniatures, sauvegardes...)
- Detection des gros fichiers, fichiers anciens, dossiers vides, extensions rares
- Recherche et filtres multi-criteres
- Suppression securisee : corbeille systeme ou dossier de quarantaine local, jamais
  de suppression definitive silencieuse
- Historique complet des operations avec restauration
- Export des statistiques en CSV, JSON et PDF
- Parametres persistants (dossiers exclus, extensions ignorees, tailles, theme, langue...)

## Installation

```bash
python -m venv .venv
# Windows : .venv\Scripts\activate
# macOS/Linux : source .venv/bin/activate

pip install -r requirements.txt
```

> `tkinterdnd2` (glisser-deposer natif) et `python-magic-bin` (Windows) sont
> optionnels : l'application reste pleinement fonctionnelle sans eux (le
> glisser-deposer retombe alors sur le bouton "Choisir un dossier").

## Lancement

```bash
python main.py
```

## Icone de l'application

Placez un fichier `FileCleaner.ico` dans le dossier `/icons` : il sera utilise
pour la fenetre et, lors de l'empaquetage, pour l'executable et la barre des
taches (voir `build.py`).

## Empaquetage en executable Windows (.exe)

```bash
pip install pyinstaller
python build.py
```

Le fichier `dist/FileCleaner.exe` embarque l'icone `icons/FileCleaner.ico`
pour l'executable et la barre des taches.

## Architecture

```
/ui        interface CustomTkinter (fenetre principale, dialogues, widgets)
/core      scan recursif multithread, modeles de donnees, categorisation
/analysis  doublons, fichiers inutiles, gros fichiers, anciens, dossiers vides,
           extensions rares, recherche
/cleaner   renommage intelligent, execution securisee des actions (suppression,
           deplacement, renommage)
/history   journal des operations et restauration
/export    export CSV / JSON / PDF
/utils     configuration, logs, fonctions utilitaires
/icons     FileCleaner.ico
/logs      journaux d'execution (generes automatiquement)
/config    parametres utilisateur persistants (settings.json)
```

## Securite

- Aucune suppression definitive automatique : passage par la corbeille systeme
  ou un dossier de quarantaine reversible
- Confirmation systematique avant toute suppression (desactivable dans les
  parametres, a vos risques)
- Journal detaille de toutes les operations, avec restauration depuis l'onglet
  Historique
- Fonctionnement entierement hors ligne, aucun service cloud, aucun envoi de
  donnees
