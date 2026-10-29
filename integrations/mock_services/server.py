"""Local, deterministic DHL/Stripe/PayPal/label-printer sandbox.

This server is for integration tests only. It never contacts DHL, Stripe, PayPal,
or a physical printer and binds to loopback by default.
"""

from __future__ import annotations

import argparse
import base64
import json
from pathlib import Path
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from integrations.invoice_pdf import generate_invoice_pdf


class MockState:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.shipments: dict[str, dict] = {}
        self.payment_intents: dict[str, dict] = {}
        self.paypal_orders: dict[str, dict] = {}
        self.print_jobs: list[dict] = []


def handler_for(state: MockState):
    class Handler(BaseHTTPRequestHandler):
        server_version = "ferroshop-mock-services/0.1"

        def log_message(self, _format: str, *_args) -> None:
            return

        def send_json(self, status: int, value: dict) -> None:
            body = json.dumps(value, separators=(",", ":")).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def send_bytes(self, status: int, content_type: str, body: bytes) -> None:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def read_body(self) -> bytes:
            length = int(self.headers.get("Content-Length", "0"))
            if length > 1_000_000:
                raise ValueError("request body too large")
            return self.rfile.read(length)

        def read_json(self) -> dict:
            value = json.loads(self.read_body() or b"{}")
            if not isinstance(value, dict):
                raise ValueError("expected a JSON object")
            return value

        def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
            path = urlparse(self.path).path
            if path == "/health":
                self.send_json(200, {"status": "ok", "mode": "mock"})
                return

            if path == "/invoices/pdf":
                self.send_json(405, {"error": "method_not_allowed"})
                return

            parts = path.strip("/").split("/")
            if len(parts) == 3 and parts[:2] == ["dhl", "shipments"]:
                with state.lock:
                    shipment = state.shipments.get(parts[2])
                self.send_json(200, shipment) if shipment else self.send_json(404, {"error": "shipment_not_found"})
                return
            if path.startswith("/stripe/v1/payment_intents/"):
                payment_id = path.rsplit("/", 1)[1]
                with state.lock:
                    payment = state.payment_intents.get(payment_id)
                self.send_json(200, payment) if payment else self.send_json(404, {"error": "payment_intent_not_found"})
                return
            if path.startswith("/paypal/v2/checkout/orders/"):
                order_id = path.rsplit("/", 1)[1]
                with state.lock:
                    order = state.paypal_orders.get(order_id)
                self.send_json(200, order) if order else self.send_json(404, {"error": "order_not_found"})
                return
            if len(parts) == 3 and parts[:2] == ["dhl", "labels"]:
                label_id = parts[2]
                with state.lock:
                    shipment = state.shipments.get(label_id)
                if shipment is None:
                    self.send_json(404, {"error": "shipment_not_found"})
                    return
                fmt = parse_qs(urlparse(self.path).query).get("format", ["zpl"])[0]
                if fmt == "zpl":
                    self.send_bytes(200, "application/zpl", shipment["labelZpl"].encode())
                elif fmt == "pdf":
                    self.send_bytes(200, "application/pdf", base64.b64decode(shipment["labelPdfBase64"]))
                else:
                    self.send_json(400, {"error": "unsupported_label_format", "supported": ["pdf", "zpl"]})
                return
            self.send_json(404, {"error": "not_found"})

        def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
            try:
                self.route_post(urlparse(self.path).path)
            except (ValueError, json.JSONDecodeError) as exc:
                self.send_json(400, {"error": "invalid_request", "detail": str(exc)})

        def route_post(self, path: str) -> None:
            if path == "/dhl/shipments":
                body = self.read_json()
                for field in ("reference", "recipient", "address", "weightGrams"):
                    if not body.get(field):
                        self.send_json(422, {"error": "missing_field", "field": field})
                        return
                shipment_id = f"shp_{uuid4().hex[:12]}"
                tracking = f"00340434{uuid4().hex[:12].upper()}"
                shipment = {
                    "id": shipment_id,
                    "reference": str(body["reference"]),
                    "trackingNumber": tracking,
                    "status": "created",
                    "labelUrl": f"/dhl/labels/{shipment_id}?format=pdf",
                    "labelZpl": f"^XA^FO40,40^A0N,36,36^FD{tracking}^FS^FO40,100^A0N,28,28^FD{str(body['recipient'])[:40]}^FS^XZ",
                    # A tiny valid PDF header keeps the mock payload deterministic.
                    "labelPdfBase64": base64.b64encode(b"%PDF-1.4\n% mock DHL label\n%%EOF\n").decode(),
                }
                with state.lock:
                    state.shipments[shipment_id] = shipment
                self.send_json(201, shipment)
                return
            if path.startswith("/dhl/shipments/") and path.endswith("/cancel"):
                shipment_id = path.split("/")[3]
                with state.lock:
                    shipment = state.shipments.get(shipment_id)
                    if shipment:
                        shipment["status"] = "cancelled"
                self.send_json(200, shipment) if shipment else self.send_json(404, {"error": "shipment_not_found"})
                return
            if path == "/stripe/v1/payment_intents":
                form = parse_qs(self.read_body().decode())
                amount = form.get("amount", [""])[0]
                currency = form.get("currency", [""])[0].lower()
                if not amount.isdigit() or int(amount) <= 0 or len(currency) != 3:
                    self.send_json(400, {"error": {"type": "invalid_request_error", "message": "amount and ISO currency are required"}})
                    return
                payment_id = f"pi_mock_{uuid4().hex[:12]}"
                payment = {
                    "id": payment_id,
                    "object": "payment_intent",
                    "amount": int(amount),
                    "currency": currency,
                    "status": "requires_payment_method",
                    "client_secret": f"{payment_id}_secret_mock",
                    "livemode": False,
                }
                with state.lock:
                    state.payment_intents[payment_id] = payment
                self.send_json(200, payment)
                return
            if path == "/paypal/v2/checkout/orders":
                body = self.read_json()
                purchase_units = body.get("purchase_units", [])
                if not purchase_units or not isinstance(purchase_units, list):
                    self.send_json(422, {"name": "INVALID_REQUEST", "message": "purchase_units is required"})
                    return
                order_id = f"PAYPAL-MOCK-{uuid4().hex[:12].upper()}"
                order = {
                    "id": order_id,
                    "status": "CREATED",
                    "intent": body.get("intent", "CAPTURE"),
                    "purchase_units": purchase_units,
                    "links": [{"rel": "approve", "href": f"http://127.0.0.1:8787/mock-paypal/approve/{order_id}"}],
                }
                with state.lock:
                    state.paypal_orders[order_id] = order
                self.send_json(201, order)
                return
            if path.startswith("/paypal/v2/checkout/orders/") and path.endswith("/capture"):
                order_id = path.split("/")[5]
                with state.lock:
                    order = state.paypal_orders.get(order_id)
                    if order:
                        order["status"] = "COMPLETED"
                self.send_json(201, order) if order else self.send_json(404, {"name": "RESOURCE_NOT_FOUND"})
                return
            if path == "/printer/labels/zpl":
                body = self.read_body().decode()
                if not body.startswith("^XA") or not body.rstrip().endswith("^XZ"):
                    self.send_json(422, {"error": "invalid_zpl_label"})
                    return
                job = {"id": f"print_{uuid4().hex[:12]}", "status": "accepted", "bytes": len(body.encode())}
                with state.lock:
                    state.print_jobs.append(job | {"payload": body})
                self.send_json(202, job)
                return
            if path == "/invoices/pdf":
                invoice = self.read_json()
                pdf = generate_invoice_pdf(invoice)
                self.send_bytes(200, "application/pdf", pdf)
                return
            self.send_json(404, {"error": "not_found"})

    return Handler


def create_server(host: str = "127.0.0.1", port: int = 8787) -> ThreadingHTTPServer:
    if host not in {"127.0.0.1", "localhost", "::1"}:
        raise ValueError("mock services may only bind to loopback")
    return ThreadingHTTPServer((host, port), handler_for(MockState()))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8787)
    args = parser.parse_args()
    server = create_server(args.host, args.port)
    print(f"Mock integrations listening on http://{args.host}:{server.server_port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
