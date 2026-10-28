-- Store-API: sales channel defaults and persisted context tokens.

ALTER TABLE sales_channel
    ADD COLUMN customer_group_id  uuid NOT NULL DEFAULT '0191a6f0f4a07c7e9b1c2d3e4f5a6e01' REFERENCES customer_group (id),
    ADD COLUMN country_id         uuid NOT NULL DEFAULT '0191a6f0f4a07c7e9b1c2d3e4f5a7001' REFERENCES country (id),
    ADD COLUMN payment_method_id  uuid NOT NULL DEFAULT '0191a6f0f4a07c7e9b1c2d3e4f5a7101' REFERENCES payment_method (id),
    ADD COLUMN shipping_method_id uuid NOT NULL DEFAULT '0191a6f0f4a07c7e9b1c2d3e4f5a7201' REFERENCES shipping_method (id);

-- What a `sw-context-token` stands for: chosen language and currency, later customer and cart.
CREATE TABLE sales_channel_api_context (
    token            text PRIMARY KEY,
    sales_channel_id uuid        NOT NULL REFERENCES sales_channel (id) ON DELETE CASCADE,
    payload          jsonb       NOT NULL DEFAULT '{}',
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sales_channel_api_context_updated_idx ON sales_channel_api_context (updated_at);
