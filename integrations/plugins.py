"""Sandbox clients implementing ferroshop's shipping/payment/print boundary.

These clients speak only to the local mock contract. Production provider clients
belong in the Rust application and must add credentials, webhooks and persistence.
"""

import json
from urllib.request import Request, urlopen


class MockPluginClient:
    def __init__(self, base_url: str = "http://127.0.0.1:8787") -> None:
        if not base_url.startswith("http://127.0.0.1:") and not base_url.startswith("http://localhost:"):
            raise ValueError("sandbox plugins may only target the loopback mock server")
        self.base_url = base_url.rstrip("/")

    def request(self, path: str, *, method: str = "GET", body: bytes | dict | None = None, content_type: str = "application/json"):
        if isinstance(body, dict):
            body = json.dumps(body).encode()
        request = Request(self.base_url + path, data=body, method=method)
        if body is not None:
            request.add_header("Content-Type", content_type)
        with urlopen(request, timeout=5) as response:
            payload = response.read()
            response_type = response.headers.get("Content-Type", "")
            return payload if response_type.startswith("application/pdf") or response_type.startswith("application/zpl") else json.loads(payload)


class DhlShippingPlugin(MockPluginClient):
    def create_shipment(self, *, reference: str, recipient: str, address: dict, weight_grams: int) -> dict:
        return self.request("/dhl/shipments", method="POST", body={
            "reference": reference,
            "recipient": recipient,
            "address": address,
            "weightGrams": weight_grams,
        })

    def tracking(self, shipment_id: str) -> dict:
        return self.request(f"/dhl/shipments/{shipment_id}")

    def label(self, shipment_id: str, fmt: str = "pdf") -> bytes:
        if fmt not in {"pdf", "zpl"}:
            raise ValueError("label format must be pdf or zpl")
        return self.request(f"/dhl/labels/{shipment_id}?format={fmt}")

    def cancel(self, shipment_id: str) -> dict:
        return self.request(f"/dhl/shipments/{shipment_id}/cancel", method="POST", body={})


class StripeSandboxPlugin(MockPluginClient):
    def create_payment_intent(self, *, amount_minor: int, currency: str = "EUR") -> dict:
        from urllib.parse import urlencode

        form = urlencode({"amount": str(amount_minor), "currency": currency.lower(), "payment_method_types[0]": "card"}).encode()
        return self.request("/stripe/v1/payment_intents", method="POST", body=form, content_type="application/x-www-form-urlencoded")

    def payment_intent(self, payment_id: str) -> dict:
        return self.request(f"/stripe/v1/payment_intents/{payment_id}")


class PayPalSandboxPlugin(MockPluginClient):
    def create_order(self, *, amount: str, currency: str = "EUR") -> dict:
        return self.request("/paypal/v2/checkout/orders", method="POST", body={
            "intent": "CAPTURE",
            "purchase_units": [{"amount": {"currency_code": currency.upper(), "value": amount}}],
        })

    def capture(self, order_id: str) -> dict:
        return self.request(f"/paypal/v2/checkout/orders/{order_id}/capture", method="POST", body={})


class ZplLabelPrinterPlugin(MockPluginClient):
    def print_label(self, zpl: str) -> dict:
        return self.request("/printer/labels/zpl", method="POST", body=zpl.encode(), content_type="application/zpl")


class InvoicePdfPlugin(MockPluginClient):
    def render(self, invoice_snapshot: dict) -> bytes:
        return self.request("/invoices/pdf", method="POST", body=invoice_snapshot)
