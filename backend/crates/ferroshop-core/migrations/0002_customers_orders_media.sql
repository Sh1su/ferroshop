-- Milestone 2: master data, customers, orders, media, integration roles.

CREATE TABLE country (
    id         uuid PRIMARY KEY,
    iso        text        NOT NULL UNIQUE,
    iso3       text,
    active     boolean     NOT NULL DEFAULT true,
    position   integer     NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);

CREATE TABLE country_translation (
    country_id  uuid        NOT NULL REFERENCES country (id) ON DELETE CASCADE,
    language_id uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name        text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz,
    PRIMARY KEY (country_id, language_id)
);

CREATE TABLE customer_group (
    id               uuid PRIMARY KEY,
    display_gross    boolean     NOT NULL DEFAULT true,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz
);

CREATE TABLE customer_group_translation (
    customer_group_id uuid        NOT NULL REFERENCES customer_group (id) ON DELETE CASCADE,
    language_id       uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name              text,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz,
    PRIMARY KEY (customer_group_id, language_id)
);

CREATE TABLE payment_method (
    id         uuid PRIMARY KEY,
    technical_name text      NOT NULL UNIQUE,
    active     boolean     NOT NULL DEFAULT true,
    position   integer     NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);

CREATE TABLE payment_method_translation (
    payment_method_id uuid        NOT NULL REFERENCES payment_method (id) ON DELETE CASCADE,
    language_id       uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name              text,
    description       text,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz,
    PRIMARY KEY (payment_method_id, language_id)
);

CREATE TABLE shipping_method (
    id         uuid PRIMARY KEY,
    technical_name text      NOT NULL UNIQUE,
    active     boolean     NOT NULL DEFAULT true,
    position   integer     NOT NULL DEFAULT 1,
    price      jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);

CREATE TABLE shipping_method_translation (
    shipping_method_id uuid        NOT NULL REFERENCES shipping_method (id) ON DELETE CASCADE,
    language_id        uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name               text,
    description        text,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz,
    PRIMARY KEY (shipping_method_id, language_id)
);

-- Customers --------------------------------------------------------------------

CREATE SEQUENCE customer_number_seq START 10001;

CREATE TABLE customer (
    id                          uuid PRIMARY KEY,
    customer_number             text        NOT NULL UNIQUE DEFAULT nextval('customer_number_seq')::text,
    group_id                    uuid        NOT NULL REFERENCES customer_group (id),
    sales_channel_id            uuid        NOT NULL REFERENCES sales_channel (id),
    language_id                 uuid        NOT NULL REFERENCES language (id),
    default_payment_method_id   uuid REFERENCES payment_method (id) ON DELETE SET NULL,
    title                       text,
    first_name                  text        NOT NULL,
    last_name                   text        NOT NULL,
    company                     text,
    email                       text        NOT NULL,
    password                    text,
    active                      boolean     NOT NULL DEFAULT true,
    guest                       boolean     NOT NULL DEFAULT false,
    birthday                    date,
    last_login                  timestamptz,
    default_billing_address_id  uuid,
    default_shipping_address_id uuid,
    created_at                  timestamptz NOT NULL DEFAULT now(),
    updated_at                  timestamptz
);
CREATE UNIQUE INDEX customer_email_unique ON customer (lower(email)) WHERE NOT guest;

CREATE TABLE customer_address (
    id                       uuid PRIMARY KEY,
    customer_id              uuid        NOT NULL REFERENCES customer (id) ON DELETE CASCADE,
    country_id               uuid        NOT NULL REFERENCES country (id),
    first_name               text        NOT NULL,
    last_name                text        NOT NULL,
    company                  text,
    street                   text        NOT NULL,
    additional_address_line1 text,
    zipcode                  text,
    city                     text        NOT NULL,
    phone_number             text,
    created_at               timestamptz NOT NULL DEFAULT now(),
    updated_at               timestamptz
);
CREATE INDEX customer_address_customer_idx ON customer_address (customer_id);

ALTER TABLE customer
    ADD FOREIGN KEY (default_billing_address_id) REFERENCES customer_address (id) ON DELETE SET NULL,
    ADD FOREIGN KEY (default_shipping_address_id) REFERENCES customer_address (id) ON DELETE SET NULL;

-- Orders (`order` is reserved in SQL, hence `sales_order`) ----------------------

CREATE SEQUENCE order_number_seq START 10001;

CREATE TABLE sales_order (
    id                  uuid PRIMARY KEY,
    order_number        text        NOT NULL UNIQUE DEFAULT nextval('order_number_seq')::text,
    order_date_time     timestamptz NOT NULL DEFAULT now(),
    sales_channel_id    uuid        NOT NULL REFERENCES sales_channel (id),
    currency_id         uuid        NOT NULL REFERENCES currency (id),
    currency_factor     numeric     NOT NULL DEFAULT 1,
    language_id         uuid        NOT NULL REFERENCES language (id),
    customer_id         uuid REFERENCES customer (id) ON DELETE SET NULL,
    customer_number     text,
    customer_email      text        NOT NULL,
    customer_first_name text        NOT NULL,
    customer_last_name  text        NOT NULL,
    customer_company    text,
    billing_address_id  uuid,
    shipping_address_id uuid,
    payment_method_id   uuid REFERENCES payment_method (id),
    shipping_method_id  uuid REFERENCES shipping_method (id),
    amount_total        numeric     NOT NULL DEFAULT 0,
    amount_net          numeric     NOT NULL DEFAULT 0,
    position_price      numeric     NOT NULL DEFAULT 0,
    shipping_total      numeric     NOT NULL DEFAULT 0,
    tax_status          text        NOT NULL DEFAULT 'gross',
    state               text        NOT NULL DEFAULT 'open'
        CHECK (state IN ('open', 'in_progress', 'completed', 'cancelled')),
    payment_state       text        NOT NULL DEFAULT 'open'
        CHECK (payment_state IN ('open', 'paid', 'refunded', 'cancelled')),
    delivery_state      text        NOT NULL DEFAULT 'open'
        CHECK (delivery_state IN ('open', 'shipped', 'delivered', 'returned', 'cancelled')),
    customer_comment    text,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz
);
CREATE INDEX sales_order_customer_idx ON sales_order (customer_id);
CREATE INDEX sales_order_date_idx ON sales_order (order_date_time);

CREATE TABLE order_address (
    id           uuid PRIMARY KEY,
    order_id     uuid        NOT NULL REFERENCES sales_order (id) ON DELETE CASCADE,
    country_id   uuid        NOT NULL REFERENCES country (id),
    first_name   text        NOT NULL,
    last_name    text        NOT NULL,
    company      text,
    street       text        NOT NULL,
    zipcode      text,
    city         text        NOT NULL,
    phone_number text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz
);

ALTER TABLE sales_order
    ADD FOREIGN KEY (billing_address_id) REFERENCES order_address (id) ON DELETE SET NULL,
    ADD FOREIGN KEY (shipping_address_id) REFERENCES order_address (id) ON DELETE SET NULL;

CREATE TABLE order_line_item (
    id          uuid PRIMARY KEY,
    order_id    uuid        NOT NULL REFERENCES sales_order (id) ON DELETE CASCADE,
    product_id  uuid REFERENCES product (id) ON DELETE SET NULL,
    type        text        NOT NULL DEFAULT 'product',
    label       text        NOT NULL,
    quantity    integer     NOT NULL CHECK (quantity > 0),
    unit_price  numeric     NOT NULL,
    total_price numeric     NOT NULL,
    tax_rate    numeric     NOT NULL DEFAULT 0,
    position    integer     NOT NULL DEFAULT 0,
    payload     jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz
);
CREATE INDEX order_line_item_order_idx ON order_line_item (order_id);

-- Media --------------------------------------------------------------------------

CREATE TABLE media (
    id             uuid PRIMARY KEY,
    file_name      text,
    file_extension text,
    mime_type      text,
    file_size      bigint,
    path           text UNIQUE,
    width          integer,
    height         integer,
    thumbnails     jsonb       NOT NULL DEFAULT '[]',
    uploaded_at    timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz
);

CREATE TABLE media_translation (
    media_id      uuid        NOT NULL REFERENCES media (id) ON DELETE CASCADE,
    language_id   uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    title         text,
    alt           text,
    custom_fields jsonb,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz,
    PRIMARY KEY (media_id, language_id)
);

CREATE TABLE product_media (
    id         uuid PRIMARY KEY,
    product_id uuid        NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    media_id   uuid        NOT NULL REFERENCES media (id) ON DELETE CASCADE,
    position   integer     NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);
CREATE INDEX product_media_product_idx ON product_media (product_id);

ALTER TABLE product ADD COLUMN cover_id uuid REFERENCES product_media (id) ON DELETE SET NULL;
ALTER TABLE product_manufacturer ADD COLUMN media_id uuid REFERENCES media (id) ON DELETE SET NULL;

-- Integration roles ----------------------------------------------------------------

CREATE TABLE integration_role (
    integration_id uuid NOT NULL REFERENCES integration (id) ON DELETE CASCADE,
    acl_role_id    uuid NOT NULL REFERENCES acl_role (id) ON DELETE CASCADE,
    PRIMARY KEY (integration_id, acl_role_id)
);

-- Default data -----------------------------------------------------------------------

INSERT INTO customer_group (id) VALUES ('0191a6f0f4a07c7e9b1c2d3e4f5a6e01');
INSERT INTO customer_group_translation (customer_group_id, language_id, name) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6e01', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Standard customer group'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6e01', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Standard-Kundengruppe');

INSERT INTO country (id, iso, iso3, position) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7001', 'DE', 'DEU', 1),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7002', 'AT', 'AUT', 2),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7003', 'CH', 'CHE', 3),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7004', 'NL', 'NLD', 4),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7005', 'FR', 'FRA', 5),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7006', 'GB', 'GBR', 6),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7007', 'US', 'USA', 7);
INSERT INTO country_translation (country_id, language_id, name)
SELECT c.id, l.id, CASE l.locale_code
        WHEN 'de-DE' THEN (ARRAY['Deutschland','Österreich','Schweiz','Niederlande','Frankreich','Vereinigtes Königreich','Vereinigte Staaten'])[c.position]
        ELSE (ARRAY['Germany','Austria','Switzerland','Netherlands','France','United Kingdom','United States'])[c.position]
    END
FROM country c CROSS JOIN language l;

INSERT INTO payment_method (id, technical_name, position) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7101', 'prepayment', 1),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7102', 'invoice', 2),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7103', 'cash_on_delivery', 3);
INSERT INTO payment_method_translation (payment_method_id, language_id, name) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7101', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Paid in advance'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7101', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Vorkasse'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7102', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Invoice'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7102', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Rechnung'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7103', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Cash on delivery'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7103', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Nachnahme');

INSERT INTO shipping_method (id, technical_name, position, price) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7201', 'standard', 1,
     '[{"currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca", "gross": 4.99, "net": 4.19, "linked": true}]'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7202', 'express', 2,
     '[{"currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca", "gross": 9.99, "net": 8.39, "linked": true}]');
INSERT INTO shipping_method_translation (shipping_method_id, language_id, name) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7201', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Standard'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7201', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Standard'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7202', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Express'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7202', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Express');
