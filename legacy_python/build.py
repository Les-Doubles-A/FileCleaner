"""
Script d'empaquetage de File Cleaner en executable autonome via PyInstaller.

Usage :
    pip install pyinstaller
    python build.py
"""
from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
ICON_PATH = ROOT / "icons" / "FileCleaner.ico"


def main() -> None:
    if not ICON_PATH.exists():
        print(f"ATTENTION : icone introuvable a {ICON_PATH}. "
              "L'executable sera genere sans icone personnalisee.")

    args = [
        sys.executable, "-m", "PyInstaller",
        "--noconfirm",
        "--windowed",
        "--onefile",
        "--name", "FileCleaner",
        "--add-data", f"{ROOT / 'icons'}{';' if sys.platform.startswith('win') else ':'}icons",
    ]
    if ICON_PATH.exists():
        args += ["--icon", str(ICON_PATH)]
    args.append(str(ROOT / "main.py"))

    print("Commande PyInstaller :", " ".join(args))
    subprocess.run(args, check=True)

    dist_exe = ROOT / "dist" / ("FileCleaner.exe" if sys.platform.startswith("win") else "FileCleaner")
    if dist_exe.exists():
        print(f"\nExecutable genere : {dist_exe}")
    else:
        print("\nGeneration terminee, verifiez le dossier dist/.")


if __name__ == "__main__":
    main()
