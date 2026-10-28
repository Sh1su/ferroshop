-- Store-API carts. Only line item references are stored; prices are recalculated on every read.

CREATE TABLE cart (
    token            text PRIMARY KEY,
    sales_channel_id uuid        NOT NULL REFERENCES sales_channel (id) ON DELETE CASCADE,
    line_items       jsonb       NOT NULL DEFAULT '[]',
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX cart_updated_idx ON cart (updated_at);
