"""Small dependency-free invoice PDF renderer for sandbox/demo workflows.

This intentionally does not claim jurisdiction-specific legal compliance. The
caller must pass an immutable issued-invoice snapshot and configure the
applicable tax rules before using this for real invoices.
"""

from decimal import Decimal, ROUND_HALF_UP
import unicodedata


CENT = Decimal("0.01")


def _money(value: Decimal) -> str:
    return f"{value.quantize(CENT, rounding=ROUND_HALF_UP):,.2f}"


def _latin_text(value: object) -> str:
    raw = str(value).replace("€", "EUR")
    return unicodedata.normalize("NFKD", raw).encode("ascii", "ignore").decode("ascii")


def _pdf_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def _page(lines: list[str]) -> bytes:
    commands = ["BT", "/F1 10 Tf", "50 790 Td"]
    for index, line in enumerate(lines):
        if index:
            commands.append("0 -16 Td")
        commands.append(f"({_pdf_escape(_latin_text(line))}) Tj")
    commands.append("ET")
    stream = "\n".join(commands).encode("ascii")
    objects = [
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Type /Catalog /Pages 3 0 R >>",
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 3 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 1 0 R >> >> /Contents 5 0 R >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
    ]
    output = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for number, obj in enumerate(objects, start=1):
        offsets.append(len(output))
        output.extend(f"{number} 0 obj\n".encode())
        output.extend(obj)
        output.extend(b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        output.extend(f"{offset:010d} 00000 n \n".encode())
    output.extend(
        f"trailer\n<< /Size {len(offsets)} /Root 2 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    )
    return bytes(output)


def generate_invoice_pdf(invoice: dict) -> bytes:
    """Render a one-page PDF from a frozen invoice snapshot.

    Required fields: invoiceNumber, issueDate, currency, seller, buyer, lines.
    Each line has label, quantity, unitNet, and taxRate (percent).
    """
    for required in ("invoiceNumber", "issueDate", "currency", "seller", "buyer", "lines"):
        if required not in invoice:
            raise ValueError(f"missing invoice field: {required}")
    currency = str(invoice["currency"]).upper()
    if len(currency) != 3 or not currency.isalpha():
        raise ValueError("currency must be an ISO 4217 code")
    seller = invoice["seller"]
    buyer = invoice["buyer"]
    lines = invoice["lines"]
    if not isinstance(seller, dict) or not seller.get("name"):
        raise ValueError("seller.name is required")
    if not isinstance(buyer, dict) or not buyer.get("name"):
        raise ValueError("buyer.name is required")
    if not isinstance(lines, list) or not lines:
        raise ValueError("at least one invoice line is required")

    rows: list[tuple[str, Decimal, Decimal, Decimal, Decimal]] = []
    tax_totals: dict[Decimal, Decimal] = {}
    net_total = Decimal("0")
    for line in lines:
        try:
            quantity = Decimal(str(line["quantity"]))
            unit_net = Decimal(str(line["unitNet"]))
            rate = Decimal(str(line["taxRate"]))
        except (KeyError, ArithmeticError, ValueError) as exc:
            raise ValueError("invoice lines require numeric quantity, unitNet, taxRate") from exc
        if quantity <= 0 or unit_net < 0 or rate < 0 or rate > 100:
            raise ValueError("invoice line values are outside supported ranges")
        net = (quantity * unit_net).quantize(CENT, rounding=ROUND_HALF_UP)
        tax = (net * rate / Decimal("100")).quantize(CENT, rounding=ROUND_HALF_UP)
        rows.append((str(line.get("label", "Item")), quantity, unit_net, rate, net))
        net_total += net
        tax_totals[rate] = tax_totals.get(rate, Decimal("0")) + tax

    tax_total = sum(tax_totals.values(), Decimal("0"))
    gross_total = net_total + tax_total
    document_lines = [
        "INVOICE",
        f"Invoice number: {invoice['invoiceNumber']}",
        f"Issue date: {invoice['issueDate']}",
        f"Order: {invoice.get('orderNumber', '-')}",
        "",
        f"Seller: {seller['name']}",
        *[_latin_text(line) for line in seller.get("address", [])],
        f"Tax ID: {seller.get('taxId', '-')}",
        "",
        f"Bill to: {buyer['name']}",
        *[_latin_text(line) for line in buyer.get("address", [])],
        "",
        "Item | Qty | Unit net | VAT | Line net",
    ]
    for label, quantity, unit_net, rate, net in rows:
        document_lines.append(
            f"{label[:40]} | {_money(quantity)} | {_money(unit_net)} {currency} | {rate}% | {_money(net)} {currency}"
        )
    document_lines.append("")
    document_lines.append(f"Net total: {_money(net_total)} {currency}")
    for rate in sorted(tax_totals):
        document_lines.append(f"VAT {rate}%: {_money(tax_totals[rate])} {currency}")
    document_lines.extend(
        [
            f"VAT total: {_money(tax_total)} {currency}",
            f"Gross total: {_money(gross_total)} {currency}",
            "",
            "Generated from the issued invoice snapshot.",
        ]
    )
    return _page(document_lines)
