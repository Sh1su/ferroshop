# ferroshop

An open-source e-commerce backend written in Rust, modelled on the
[Shopware 6 Community Edition](https://github.com/shopware/shopware) (MIT).

> ferroshop is an independent project. It is not affiliated with or endorsed by shopware AG.
> "Shopware" is a trademark of shopware AG.

## Goals

- **Store-API drop-in compatible with Shopware** (phase 2), so
  [Shopware Frontends](https://frontends.shopware.com) can run against ferroshop unchanged.
- **Admin-API modelled on Shopware**: the same generic entity endpoints, criteria syntax
  (filters, sorting, associations, aggregations, includes) and error format.
- Its own clean PostgreSQL schema, a single binary and a Vue 3 Administration.

## Status: milestone 2

| Area | State |
| --- | --- |
| Generic entity layer: CRUD, criteria → SQL, translations, variant inheritance, audit log | ✅ |
| Admin-API: OAuth2 (password, refresh token, client credentials), `/api/search/{entity}`, CRUD, OpenAPI | ✅ |
| Entities: product, category, manufacturer, tax, currency, language, sales channel, user, ACL role, integration | ✅ |
| Administration: login, product list, product detail (translations, prices, variants, categories), de/en | ✅ |
| Customers, orders (state machines), media (local/S3, thumbnails), roles & permissions | ✅ |
| Administration: customers, orders, media library, master data, roles, profile | ✅ |
| Store-API: context, products, category listings, search, navigation, master data, calculated prices | ✅ |
| Store-API: cart (line items, purchase rules, closeout stock, shipping costs, tax totals) | ✅ |
| Store-API: checkout/order, customer account, SEO URLs, CMS pages; Shopware Frontends end-to-end | next |

## Quick start

Requirements: Rust (stable), Node 24, Docker.

```sh
cp .env.example .env
docker compose up -d postgres

cd backend
cargo run -- migrate
cargo run -- user:create admin --email admin@example.com --password shopware1
cargo run -- demo-data
cargo run -- serve          # http://localhost:8000/api

cd ../admin
npm ci
npm run dev                 # http://localhost:5173/admin/  (proxies /api)
```

Or everything in Docker:

```sh
docker compose --profile app up --build
docker compose exec ferroshop ferroshop user:create admin --email admin@example.com --password shopware1
# Administration: http://localhost:8000/admin/
```

## Admin-API examples

```sh
TOKEN=$(curl -s localhost:8000/api/oauth/token -H 'content-type: application/json' \
  -d '{"grant_type":"password","client_id":"administration","username":"admin","password":"shopware1"}' | jq -r .access_token)

curl -s localhost:8000/api/search/product -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{
  "limit": 10,
  "filter": [{ "type": "equals", "field": "categories.name", "value": "Shoes" }],
  "sort": [{ "field": "name", "order": "ASC" }],
  "associations": { "manufacturer": {} },
  "aggregations": [{ "name": "stock", "type": "stats", "field": "stock" }],
  "total-count-mode": 1
}'
```

Headers: `sw-language-id` selects the content language, `sw-inheritance: 1` resolves variant
fields from their parent. The OpenAPI document is at `/api/_info/openapi3.json`; the Administration
generates its client types from it (`npm run gen:api`).

## Store-API examples

The Store-API needs the sales channel's access key (the default storefront ships with
`SWSCFERROSHOPDEFAULT00000`). Every response carries a `sw-context-token`; send it back to keep
the chosen language and currency.

```sh
H=(-H 'sw-access-key: SWSCFERROSHOPDEFAULT00000' -H 'content-type: application/json')

curl -s localhost:8000/store-api/navigation/main-navigation/main-navigation "${H[@]}" -d '{"depth": 2}'
curl -s localhost:8000/store-api/product-listing/<categoryId> "${H[@]}" -d '{"order": "price-asc", "limit": 24, "p": 1}'
curl -s 'localhost:8000/store-api/search?search=shirt' "${H[@]}" -X POST
curl -si localhost:8000/store-api/context "${H[@]}" -X PATCH -d '{"languageId": "0191a6f0f4a07c7e9b1c2d3e4f5a6b7c"}'

T=(-H "sw-context-token: $TOKEN")   # from the sw-context-token response header
curl -s localhost:8000/store-api/checkout/cart/line-item "${H[@]}" "${T[@]}" \
  -d '{"items": [{"type": "product", "referencedId": "<productId>", "quantity": 2}]}'
curl -s localhost:8000/store-api/checkout/cart "${H[@]}" "${T[@]}"
```

Carts store only references and quantities and are recalculated on every read; products that
sold out, were deactivated or changed their purchase rules are adjusted and reported once in
`errors`.

Storefront visitors only see active products, categories, countries, payment and shipping
methods, and criteria may only reach catalogue entities. Listings also take Shopware's
`manufacturer`, `min-price`, `max-price` and `no-aggregations` parameters.

## Layout

```
backend/crates/
  ferroshop-core       entity definitions (entity! macro), criteria, SQL compiler, read/write, migrations
  ferroshop-admin-api  axum router for /api: OAuth2, generic entity endpoints, OpenAPI
  ferroshop-store-api  /store-api: sales channel context, catalogue, listings, navigation
  ferroshop-cli        the `ferroshop` binary: serve, migrate, user:create, demo-data
admin/                 Vue 3 + Vite + TypeScript + Meteor Component Library
```

Adding an entity means writing a migration and an `entity!` block in
`backend/crates/ferroshop-core/src/entities.rs`. API, search, OpenAPI and schema follow from it.

## Tests

```sh
cd backend && cargo test     # needs DATABASE_URL; sqlx::test creates a database per test
                             # crates/*/tests: entity layer, milestone 2, Admin-API and Store-API over HTTP
```

## License

MIT

## Deployment via NetBird

`deploy/netbird/` runs ferroshop so that it is only reachable inside the NetBird network:
the app is published on the host's NetBird IP only and the database is not published at all.

```sh
cd deploy/netbird
cp .env.example .env     # set NETBIRD_IP, POSTGRES_PASSWORD, APP_SECRET (openssl rand -hex 32)
docker compose up -d --build
docker compose exec -e FERROSHOP_PASSWORD='…' app ferroshop user:create admin --email admin@example.com
```

Update: `git pull && docker compose up -d --build` (migrations run on start).
