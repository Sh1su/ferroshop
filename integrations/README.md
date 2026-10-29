# Checkout, shipping, invoices, and label printing

This integration boundary is intentionally sandbox-first. The repository's current seed data already provides **EUR** as its only currency and the German VAT examples **19% standard / 7% reduced**. Those rates are demo defaults for a DE shop, not legal advice or a rule for other destinations. Production shops must configure tax rules per country, tax status, and product; invoice totals must use the tax snapshot saved with each order.

## Default shop profile

| Setting | Initial value | Production rule |
| --- | --- | --- |
| Base currency | EUR, factor 1, 2 decimal places | Add currencies and rates explicitly; never reinterpret historical order amounts after a rate change. |
| Language | de-DE with en-GB available | Set per sales channel. |
| German standard VAT example | 19% | Configure the applicable rate and effective date per jurisdiction. |
| German reduced VAT example | 7% | Assign only to eligible products and regions. |
| New sales channels | Draft / inactive | Activate only after required context defaults have been validated. |
| Payment providers | Stripe and PayPal sandbox | Live mode must require explicit environment configuration and verified webhooks. |
| Shipping | DHL test adapter | Live DHL credentials and service configuration are separate from the mock server. |
| Label format | ZPL for common network label printers; PDF download also supported by the mock | Printer host, media size, and DPI must be configured per device. |

The Admin starts new storefront channels as drafts and preselects the seeded DE / EUR / standard-customer / Germany / prepayment / standard-shipping references. Currency forms start with factor `1` and 2 decimal places; new tax-rate forms start at the German standard example of `19%`. A channel can be activated after it has at least one domain configured with an absolute URL, language, and currency. Add different language/currency combinations as separate domain records.

## Invoice PDF requirements

An invoice must be issued from an immutable order snapshot, with a unique sequential invoice number, issue date, seller and buyer addresses, currency, line items, net/tax/gross totals grouped by tax rate, and references to the source order. Re-downloading the invoice must return the same content. Order changes after issuance must not rewrite it; corrections should create a credit note or a replacement document with an audit trail. The PDF output must be tested for its content and HTTP headers before production use.

## Local integration mock

The separate mock service binds only to loopback and performs no outbound calls to DHL, Stripe, or PayPal:

```sh
python3 integrations/mock_services/server.py
python3 -m unittest discover -s integrations/mock_services -p 'test_*.py' -v
```

Base URL: `http://127.0.0.1:8787`.

| Provider / device | Mock routes |
| --- | --- |
| DHL shipping | `POST /dhl/shipments`, `GET /dhl/shipments/{id}`, `POST /dhl/shipments/{id}/cancel` |
| DHL labels | `GET /dhl/labels/{id}?format=pdf` and `?format=zpl` |
| Invoice PDF renderer | `POST /invoices/pdf` with an invoice snapshot returns `application/pdf` |
| Stripe sandbox contract | `POST /stripe/v1/payment_intents`, `GET /stripe/v1/payment_intents/{id}` |
| PayPal sandbox contract | `POST /paypal/v2/checkout/orders`, `GET /paypal/v2/checkout/orders/{id}`, `POST /paypal/v2/checkout/orders/{id}/capture` |
| Label printer | `POST /printer/labels/zpl` |

Use the mock server URL in tests only. The server rejects non-loopback bind addresses. Payment responses always identify as sandbox (`livemode: false`); mock shipment labels and print jobs are generated locally.

The invoice endpoint calculates line net values and VAT grouped by rate using decimal arithmetic. It is a development renderer, not a persisted invoice-issuance workflow or a jurisdiction-certified template.

## Planned adapter boundary

Provider adapters should accept ferroshop order IDs and saved order snapshots and return normalized states: `pending`, `authorized`, `paid`, `failed`, `cancelled`, `refunded` for payments; shipment ID, tracking number, and label bytes for DHL. Store provider IDs and webhook events idempotently. Never mark an order paid because a browser redirect succeeded; verify a provider webhook or server-side capture first. Printer adapters should render label payloads and enqueue them only after explicit configuration; tests use the local ZPL sink above.

This directory establishes the mock wire contract and defaults. It does not claim that live provider credentials, production tax rules, invoice issuance, or physical printing are wired into the storefront yet.
