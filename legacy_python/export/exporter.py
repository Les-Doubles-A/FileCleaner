"""
Export des resultats d'analyse au format CSV, JSON ou PDF.

Tout l'export est local : aucun fichier n'est envoye a l'exterieur.
"""
from __future__ import annotations

import csv
import json
from datetime import datetime
from pathlib import Path

from core.models import ScanResult
from utils.helpers import format_size
from utils.logger import get_logger

logger = get_logger(__name__)


def export_csv(result: ScanResult, output_path: Path) -> Path:
    fieldnames = ["chemin", "nom", "extension", "taille_octets", "cree",
                  "modifie", "accede", "type_mime", "categorie", "sha256"]
    with open(output_path, "w", newline="", encoding="utf-8-sig") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for file_info in result.files:
            writer.writerow(file_info.to_export_dict())
    logger.info("Export CSV genere : %s", output_path)
    return output_path


def export_json(result: ScanResult, output_path: Path) -> Path:
    data = {
        "dossier_analyse": str(result.root),
        "date_analyse": result.scanned_at.isoformat(timespec="seconds"),
        "nombre_fichiers": result.total_files,
        "nombre_dossiers": result.total_folders,
        "taille_totale_octets": result.total_size,
        "taille_totale_lisible": format_size(result.total_size),
        "repartition_extensions": {
            ext: {"nombre": count, "taille_octets": size}
            for ext, (count, size) in result.extension_breakdown().items()
        },
        "repartition_categories": {
            cat: {"nombre": count, "taille_octets": size}
            for cat, (count, size) in result.category_breakdown().items()
        },
        "fichiers": [f.to_export_dict() for f in result.files],
        "erreurs": result.errors,
    }
    output_path.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
    logger.info("Export JSON genere : %s", output_path)
    return output_path


def export_pdf(result: ScanResult, output_path: Path) -> Path:
    """
    Genere un rapport PDF de synthese (statistiques principales).
    Utilise fpdf2 si disponible ; sinon, ecrit un PDF minimal "texte brut"
    encapsule manuellement pour eviter toute dependance bloquante.
    """
    try:
        from fpdf import FPDF  # type: ignore

        pdf = FPDF()
        pdf.add_page()
        pdf.set_font("Helvetica", "B", 16)
        pdf.cell(0, 10, "File Cleaner - Rapport d'analyse", new_x="LMARGIN", new_y="NEXT")

        pdf.set_font("Helvetica", "", 11)
        pdf.cell(0, 8, f"Dossier analyse : {result.root}", new_x="LMARGIN", new_y="NEXT")
        pdf.cell(0, 8, f"Date : {result.scanned_at:%d/%m/%Y %H:%M}", new_x="LMARGIN", new_y="NEXT")
        pdf.cell(0, 8, f"Fichiers : {result.total_files}", new_x="LMARGIN", new_y="NEXT")
        pdf.cell(0, 8, f"Dossiers : {result.total_folders}", new_x="LMARGIN", new_y="NEXT")
        pdf.cell(0, 8, f"Taille totale : {format_size(result.total_size)}", new_x="LMARGIN", new_y="NEXT")

        pdf.ln(4)
        pdf.set_font("Helvetica", "B", 13)
        pdf.cell(0, 8, "Repartition par categorie", new_x="LMARGIN", new_y="NEXT")
        pdf.set_font("Helvetica", "", 10)
        for cat, (count, size) in sorted(
            result.category_breakdown().items(), key=lambda kv: kv[1][1], reverse=True
        ):
            pdf.cell(0, 6, f"{cat} : {count} fichiers - {format_size(size)}", new_x="LMARGIN", new_y="NEXT")

        pdf.ln(4)
        pdf.set_font("Helvetica", "B", 13)
        pdf.cell(0, 8, "Plus gros fichiers", new_x="LMARGIN", new_y="NEXT")
        pdf.set_font("Helvetica", "", 10)
        for f in result.largest_files(15):
            pdf.multi_cell(0, 6, f"{f.name} - {format_size(f.size)} - {f.path.parent}")

        pdf.output(str(output_path))
    except ImportError:
        _export_pdf_fallback(result, output_path)

    logger.info("Export PDF genere : %s", output_path)
    return output_path


def _export_pdf_fallback(result: ScanResult, output_path: Path) -> None:
    """
    Generateur PDF minimal sans dependance externe (texte simple),
    utilise uniquement si fpdf2 n'est pas installe.
    """
    lines = [
        "File Cleaner - Rapport d'analyse",
        f"Dossier analyse : {result.root}",
        f"Date : {result.scanned_at:%d/%m/%Y %H:%M}",
        f"Fichiers : {result.total_files}",
        f"Dossiers : {result.total_folders}",
        f"Taille totale : {format_size(result.total_size)}",
        "",
        "Repartition par categorie :",
    ]
    for cat, (count, size) in sorted(
        result.category_breakdown().items(), key=lambda kv: kv[1][1], reverse=True
    ):
        lines.append(f"  - {cat} : {count} fichiers - {format_size(size)}")

    content_stream = "BT /F1 11 Tf 40 780 Td 14 TL\n"
    for line in lines:
        escaped = line.replace("\\", r"\\").replace("(", r"\(").replace(")", r"\)")
        content_stream += f"({escaped}) Tj T*\n"
    content_stream += "ET"

    objects = []
    objects.append("<< /Type /Catalog /Pages 2 0 R >>")
    objects.append("<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    objects.append(
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 4 0 R >> >> "
        "/MediaBox [0 0 612 792] /Contents 5 0 R >>"
    )
    objects.append("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    objects.append(f"<< /Length {len(content_stream)} >>\nstream\n{content_stream}\nendstream")

    pdf_parts = ["%PDF-1.4\n"]
    offsets = [0]
    body = pdf_parts[0]
    for i, obj in enumerate(objects, start=1):
        offsets.append(len(body))
        body += f"{i} 0 obj\n{obj}\nendobj\n"

    xref_offset = len(body)
    xref = f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n"
    for off in offsets[1:]:
        xref += f"{off:010} 00000 n \n"
    trailer = f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF"

    output_path.write_bytes((body + xref + trailer).encode("latin-1", errors="replace"))


def default_export_filename(extension: str, root: Path) -> str:
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    return f"filecleaner_{root.name}_{timestamp}.{extension}"
