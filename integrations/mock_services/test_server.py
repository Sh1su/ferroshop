import json
import threading
import unittest
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

from server import create_server
from integrations.plugins import DhlShippingPlugin, InvoicePdfPlugin, PayPalSandboxPlugin, StripeSandboxPlugin, ZplLabelPrinterPlugin


class MockServicesHttpTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = create_server(port=0)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f"http://127.0.0.1:{cls.server.server_port}"

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def request(self, path, method="GET", body=None, content_type="application/json"):
        data = None if body is None else (body if isinstance(body, bytes) else json.dumps(body).encode())
        request = Request(self.base + path, data=data, method=method)
        if data is not None:
            request.add_header("Content-Type", content_type)
        with urlopen(request, timeout=3) as response:
            raw = response.read()
            return response.status, response.headers.get("Content-Type"), raw

    def test_dhl_create_tracking_cancel_and_label_formats(self):
        status, _, raw = self.request("/dhl/shipments", "POST", {
            "reference": "ORDER-1001",
            "recipient": "Ada Example",
            "address": {"street": "Testweg 1", "postalCode": "10115", "city": "Berlin", "country": "DE"},
            "weightGrams": 700,
        })
        self.assertEqual(status, 201)
        shipment = json.loads(raw)
        self.assertEqual(shipment["status"], "created")
        self.assertTrue(shipment["trackingNumber"].startswith("00340434"))
        self.assertEqual(self.request(f"/dhl/shipments/{shipment['id']}")[0], 200)
        self.assertIn(b"^XA", self.request(f"/dhl/labels/{shipment['id']}?format=zpl")[2])
        self.assertTrue(self.request(f"/dhl/labels/{shipment['id']}?format=pdf")[2].startswith(b"%PDF"))
        _, _, cancelled = self.request(f"/dhl/shipments/{shipment['id']}/cancel", "POST", {})
        self.assertEqual(json.loads(cancelled)["status"], "cancelled")

    def test_stripe_sandbox_payment_intent_contract(self):
        form = urlencode({"amount": "1999", "currency": "eur", "payment_method_types[0]": "card"}).encode()
        status, _, raw = self.request("/stripe/v1/payment_intents", "POST", form, "application/x-www-form-urlencoded")
        payment = json.loads(raw)
        self.assertEqual(status, 200)
        self.assertEqual(payment["amount"], 1999)
        self.assertFalse(payment["livemode"])
        self.assertEqual(self.request(f"/stripe/v1/payment_intents/{payment['id']}")[0], 200)

    def test_paypal_sandbox_create_and_capture(self):
        status, _, raw = self.request("/paypal/v2/checkout/orders", "POST", {
            "intent": "CAPTURE",
            "purchase_units": [{"amount": {"currency_code": "EUR", "value": "19.99"}}],
        })
        order = json.loads(raw)
        self.assertEqual(status, 201)
        self.assertEqual(order["status"], "CREATED")
        _, _, raw = self.request(f"/paypal/v2/checkout/orders/{order['id']}/capture", "POST", {})
        self.assertEqual(json.loads(raw)["status"], "COMPLETED")

    def test_label_printer_accepts_zpl_jobs_only(self):
        status, _, raw = self.request("/printer/labels/zpl", "POST", b"^XA^FO40,40^FDOrder 1001^FS^XZ", "application/zpl")
        self.assertEqual(status, 202)
        self.assertEqual(json.loads(raw)["status"], "accepted")
        with self.assertRaises(HTTPError) as error:
            self.request("/printer/labels/zpl", "POST", b"not a label", "application/zpl")
        self.assertEqual(error.exception.code, 422)

    def test_invoice_pdf_contains_immutable_snapshot_totals(self):
        invoice = {
            "invoiceNumber": "RE-2026-0001",
            "orderNumber": "10001",
            "issueDate": "2026-10-29",
            "currency": "EUR",
            "seller": {"name": "Ferroshop GmbH", "address": ["Testweg 1", "10115 Berlin"], "taxId": "DE123456789"},
            "buyer": {"name": "Ada Example", "address": ["Musterstrasse 2", "10117 Berlin"]},
            "lines": [
                {"label": "Standard item", "quantity": 1, "unitNet": "100.00", "taxRate": 19},
                {"label": "Reduced item", "quantity": 2, "unitNet": "10.00", "taxRate": 7},
            ],
        }
        status, content_type, pdf = self.request("/invoices/pdf", "POST", invoice)
        self.assertEqual(status, 200)
        self.assertEqual(content_type, "application/pdf")
        self.assertTrue(pdf.startswith(b"%PDF-1.4"))
        self.assertIn(b"RE-2026-0001", pdf)
        self.assertIn(b"VAT 19%: 19.00 EUR", pdf)
        self.assertIn(b"VAT 7%: 1.40 EUR", pdf)
        self.assertIn(b"Gross total: 140.40 EUR", pdf)

    def test_mock_is_loopback_only(self):
        with self.assertRaises(ValueError):
            create_server(host="0.0.0.0", port=0)

    def test_sandbox_plugins_run_checkout_shipping_invoice_and_print_flows(self):
        dhl = DhlShippingPlugin(self.base)
        shipment = dhl.create_shipment(
            reference="ORDER-1002",
            recipient="Ada Example",
            address={"street": "Testweg 1", "postalCode": "10115", "city": "Berlin", "country": "DE"},
            weight_grams=700,
        )
        self.assertEqual(dhl.tracking(shipment["id"])["trackingNumber"], shipment["trackingNumber"])
        self.assertTrue(dhl.label(shipment["id"], "zpl").startswith(b"^XA"))
        self.assertEqual(dhl.cancel(shipment["id"])["status"], "cancelled")

        stripe = StripeSandboxPlugin(self.base)
        intent = stripe.create_payment_intent(amount_minor=2599)
        self.assertFalse(stripe.payment_intent(intent["id"])["livemode"])
        paypal = PayPalSandboxPlugin(self.base)
        order = paypal.create_order(amount="25.99")
        self.assertEqual(paypal.capture(order["id"])["status"], "COMPLETED")
        self.assertEqual(ZplLabelPrinterPlugin(self.base).print_label("^XA^FO20,20^FDOrder^FS^XZ")["status"], "accepted")

        pdf = InvoicePdfPlugin(self.base).render({
            "invoiceNumber": "RE-2026-0002", "issueDate": "2026-10-29", "currency": "EUR",
            "seller": {"name": "Ferroshop GmbH"}, "buyer": {"name": "Ada Example"},
            "lines": [{"label": "Product", "quantity": 1, "unitNet": "10.00", "taxRate": 19}],
        })
        self.assertTrue(pdf.startswith(b"%PDF-1.4"))


if __name__ == "__main__":
    unittest.main()
