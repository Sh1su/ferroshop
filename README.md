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

## Status: milestone 1

| Area | State |
| --- | --- |
| Generic entity layer: CRUD, criteria → SQL, translations, variant inheritance, audit log | ✅ |
| Admin-API: OAuth2 (password, refresh token, client credentials), `/api/search/{entity}`, CRUD, OpenAPI | ✅ |
| Entities: product, category, manufacturer, tax, currency, language, sales channel, user, ACL role, integration | ✅ |
| Administration: login, product list, product detail (translations, prices, variants, categories), de/en | ✅ |
| Customers, orders, media, master data UI, roles & permissions | milestone 2 |
| Store-API + Shopware Frontends | phase 2 |

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

## Layout

```
backend/crates/
  ferroshop-core       entity definitions (entity! macro), criteria, SQL compiler, read/write, migrations
  ferroshop-admin-api  axum router for /api: OAuth2, generic entity endpoints, OpenAPI
  ferroshop-store-api  /store-api (phase 2)
  ferroshop-cli        the `ferroshop` binary: serve, migrate, user:create, demo-data
admin/                 Vue 3 + Vite + TypeScript + Meteor Component Library
```

Adding an entity means writing a migration and an `entity!` block in
`backend/crates/ferroshop-core/src/entities.rs`. API, search, OpenAPI and schema follow from it.

## Tests

```sh
cd backend && cargo test     # needs DATABASE_URL; sqlx::test creates a database per test
```

## License

MIT
