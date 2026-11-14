#!/usr/bin/env python3
"""Writes synthetic_payslips.pdf: two payslips and a P60 for an invented
employer, as a minimal text-only PDF (stdlib only). All figures are made up
and match canned_tool_output.json.

    python make_fixtures.py
"""

from pathlib import Path

PAGES = [
    [
        "Example Widgets Ltd",
        "PAYE reference 123/AB456",
        "PAYSLIP  Pay date 27/06/2025  Period 01/06/2025 to 30/06/2025  Tax year 2025/26",
        "Total gross pay 5000.00",
        "Taxable pay 5000.00",
        "RSU notional 2000.00 (included in gross pay)",
        "PAYE tax 1100.00",
        "Employee NI 150.00",
        "Pension 150.00",
        "Net pay 1600.00",
        "Year to date: taxable pay 11000.00, tax 1700.00",
    ],
    [
        "Example Widgets Ltd",
        "PAYE reference 123/AB456",
        "PAYSLIP  Pay date 25/07/2025  Period 01/07/2025 to 31/07/2025  Tax year 2025/26",
        "Total gross pay 3000.00",
        "Taxable pay 3000.00",
        "PAYE tax 400.00",
        "Employee NI 150.00",
        "Pension 150.00",
        "Net pay 2300.00",
        "Year to date: taxable pay 14000.00, tax 2100.00",
    ],
    [
        "Example Widgets Ltd",
        "PAYE reference 123/AB456",
        "P60 End of Year Certificate  Year to 5 April 2026",
        "Pay in this employment 40000.00",
        "Tax deducted in this employment 7000.00",
    ],
]


def escape(text: str) -> str:
    for ch in ("\\", "(", ")"):
        text = text.replace(ch, "\\" + ch)
    return text


def build() -> bytes:
    objects: list[bytes] = []

    def add(body: str) -> int:
        objects.append(body.encode("latin-1"))
        return len(objects)

    catalog = add("<< /Type /Catalog /Pages 2 0 R >>")
    pages_id = add("")  # filled below
    font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    page_ids = []
    for lines in PAGES:
        ops = ["BT", "/F1 11 Tf", "14 TL", "50 780 Td"]
        for line in lines:
            ops.append(f"({escape(line)}) Tj T*")
        ops.append("ET")
        stream = "\n".join(ops)
        content = add(f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream")
        page_ids.append(
            add(
                f"<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 595 842] "
                f"/Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
            )
        )
    kids = " ".join(f"{i} 0 R" for i in page_ids)
    objects[pages_id - 1] = f"<< /Type /Pages /Kids [{kids}] /Count {len(page_ids)} >>".encode("latin-1")

    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for n, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{n} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root {catalog} 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(out)


if __name__ == "__main__":
    target = Path(__file__).with_name("synthetic_payslips.pdf")
    target.write_bytes(build())
    print(f"wrote {target}")
