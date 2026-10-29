-- A storefront sales channel can expose multiple localized and currency-specific domains.

CREATE TABLE sales_channel_domain (
    id               uuid PRIMARY KEY,
    sales_channel_id uuid        NOT NULL REFERENCES sales_channel (id) ON DELETE CASCADE,
    url              text        NOT NULL UNIQUE CHECK (url ~ '^https?://[^/[:space:]?#]+(/[^[:space:]]*)?$'),
    language_id      uuid        NOT NULL REFERENCES language (id),
    currency_id      uuid        NOT NULL REFERENCES currency (id),
    browser_language boolean     NOT NULL DEFAULT false,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz
);

CREATE INDEX sales_channel_domain_channel_idx ON sales_channel_domain (sales_channel_id);
