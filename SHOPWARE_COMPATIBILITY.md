# Shopware Community Edition compatibility

Audit date: 2026-10-29

## Compatibility baseline

This audit uses [Shopware Community Edition 6.7.15.1](https://github.com/shopware/shopware/releases/tag/v6.7.15.1), the latest 6.7 release published on the audit date, as its feature reference. The feature inventory comes from the official [Shopware Core overview](https://docs.shopware.com/en/shopware-6-en/features/shopware-core). The repository does not pin a Shopware Frontends package version or run a Frontends application end-to-end yet. Existing Store API integration tests assert local HTTP response contracts in CI; they do not compare responses against a running Shopware instance. Tracking and closing that gap is part of [issue #1](https://github.com/Sh1su/ferroshop/issues/1).


This matrix compares ferroshop with the Shopware Community Edition feature overview in the [official Shopware documentation](https://docs.shopware.com/en/shopware-6-en/features/shopware-core). It covers Shopware Core features only. Paid-plan features, hosting, support, and third-party extensions are outside this comparison. “Supported” means the repository contains an implementation; it does not imply full API or behavioral parity.

## Feature matrix

| Shopware Community Edition feature | ferroshop status | Repository evidence / remaining work |
| --- | --- | --- |
| Shopping Experiences | Partial | CMS pages, sections, blocks, slots, selected block data resolvers and category assignment exist. Page authoring is API-only; there is no visual drag-and-drop editor. |
| Roles and permissions | Partial | Admin users, roles, ACL checks, and permission-aware views exist. There is no Shopware plugin/extension permission surface. |
| Rule Builder | Missing | No rule entity, condition editor, evaluator, or rule-based shipping/tax/pricing behavior. |
| Flow Builder | Missing | No event workflow designer, action graph, or durable workflow execution. |
| Customer groups and gross/net display | Partial | Customer groups select gross/net display. Customer-specific pricing and broader group-based content rules are absent. |
| Promotions | Missing | No promotion/voucher model, administration, redemption, stacking, or discount allocation. |
| SEO | Partial | Generated product/category URLs and rename history exist. SEO template configuration and comprehensive CMS/domain routing parity are not established. |
| Search | Partial | Store API product search and listing filters exist. Shopware search configuration and full relevance/synonym behavior are absent. |
| Cross-selling | Missing | The product cross-selling route currently returns an empty result for every product. |
| Product reviews | Missing | The product reviews route currently returns an empty result; no review storage or moderation UI exists. |
| Tag management | Missing | No tag entity or tag assignment workflows are present. |
| Physical products | Partial | Product prices, stock, dimensions, purchase rules, variants, and closeout behavior exist; fulfillment parity is not complete. |
| Digital products | Missing | No downloadable product/file entitlement or delivery workflow is present. |
| Dynamic product groups | Missing | No saved product rule sets or dynamic merchandising groups are present. |
| Payment method integration | Partial | Payment methods can be configured and an offline/manual path exists. Provider integrations and payment webhooks are absent. |
| Shipping provider integration | Partial | Shipping methods and static prices exist. Provider integrations, shipping rules, and fulfillment labels are absent. |
| Category management | Supported | Categories, hierarchy, products, listing pages, and CMS page assignment exist. |
| Sales channels | Partial | Store API context, per-domain URL/language/currency, admin CRUD, and draft-before-activation validation exist. Domain-scoped snippet sets, channel-specific available-country/currency/method assignments, and full Shopware configuration parity remain open. |
| Currency and tax management | Partial | Currency records and tax classes exist. Country tax overrides with priority are used by Store API pricing; postal/region ranges, effective dates, customer context, and jurisdiction validation remain absent. |
| Multiple languages | Partial | Translated entities and language selection exist; domains can select a language and currency and browser-language preference. Import, snippet-set selection, and full fallback parity are incomplete. |
| Import/export | Missing | There is no data import/export workflow; demo-data and SEO-generation CLI commands are not substitutes. |
| Extension ecosystem | Missing | Entity definitions are compile-time and there is no plugin lifecycle, extension loader, or event/job contract. |

## Already implemented adjacent commerce flows

The repository also implements Store API context, catalog browsing, carts, customer accounts, checkout/order placement, order history, SEO URL resolution, product variants, and selected Shopware Frontends routes. These are useful compatibility foundations, but README checkmarks mean feature presence, not complete Shopware contract parity.
## Work remaining before claiming Community Edition parity

1. Complete sales-channel and commerce configuration: channel-specific assignments and broader Admin API/UI coverage (issue #9).
2. Pin a Shopware Frontends package version and add a Shopware-backed contract flow to CI (issue #1).
3. Implement rules and promotions, including deterministic cart discounts (issue #7).
4. Implement extensibility, lifecycle events, and custom fields (issue #8).
5. Add the missing Community Edition capabilities above: visual CMS editor, workflows, reviews, cross-selling, tags, dynamic product groups, digital products, and data import/export.

This order is a proposed implementation sequence, not a claim that the work is complete.
