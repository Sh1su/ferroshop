-- ferroshop initial schema.
-- Every entity table has `id uuid`, `created_at`, `updated_at`.
-- Translatable fields live in `<entity>_translation` keyed by (<entity>_id, language_id).

CREATE TABLE language (
    id          uuid PRIMARY KEY,
    parent_id   uuid REFERENCES language (id) ON DELETE SET NULL,
    name        text        NOT NULL,
    locale_code text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz
);

CREATE TABLE currency (
    id                uuid PRIMARY KEY,
    iso_code          text        NOT NULL UNIQUE,
    factor            numeric     NOT NULL DEFAULT 1,
    symbol            text        NOT NULL,
    decimal_precision integer     NOT NULL DEFAULT 2,
    position          integer     NOT NULL DEFAULT 1,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz
);

CREATE TABLE currency_translation (
    currency_id uuid        NOT NULL REFERENCES currency (id) ON DELETE CASCADE,
    language_id uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    short_name  text,
    name        text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz,
    PRIMARY KEY (currency_id, language_id)
);

CREATE TABLE tax (
    id         uuid PRIMARY KEY,
    tax_rate   numeric     NOT NULL,
    name       text        NOT NULL,
    position   integer     NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);

CREATE TABLE category (
    id         uuid PRIMARY KEY,
    parent_id  uuid REFERENCES category (id) ON DELETE CASCADE,
    position   integer     NOT NULL DEFAULT 0,
    active     boolean     NOT NULL DEFAULT true,
    visible    boolean     NOT NULL DEFAULT true,
    type       text        NOT NULL DEFAULT 'page',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);
CREATE INDEX category_parent_idx ON category (parent_id);

CREATE TABLE category_translation (
    category_id      uuid        NOT NULL REFERENCES category (id) ON DELETE CASCADE,
    language_id      uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name             text,
    description      text,
    meta_title       text,
    meta_description text,
    keywords         text,
    custom_fields    jsonb,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz,
    PRIMARY KEY (category_id, language_id)
);

CREATE TABLE sales_channel (
    id                     uuid PRIMARY KEY,
    type_id                uuid        NOT NULL,
    access_key             text        NOT NULL UNIQUE,
    language_id            uuid        NOT NULL REFERENCES language (id),
    currency_id            uuid        NOT NULL REFERENCES currency (id),
    navigation_category_id uuid REFERENCES category (id) ON DELETE SET NULL,
    active                 boolean     NOT NULL DEFAULT true,
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz
);

CREATE TABLE sales_channel_translation (
    sales_channel_id uuid        NOT NULL REFERENCES sales_channel (id) ON DELETE CASCADE,
    language_id      uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name             text,
    custom_fields    jsonb,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz,
    PRIMARY KEY (sales_channel_id, language_id)
);

CREATE TABLE product_manufacturer (
    id         uuid PRIMARY KEY,
    link       text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);

CREATE TABLE product_manufacturer_translation (
    product_manufacturer_id uuid        NOT NULL REFERENCES product_manufacturer (id) ON DELETE CASCADE,
    language_id             uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name                    text,
    description             text,
    custom_fields           jsonb,
    created_at              timestamptz NOT NULL DEFAULT now(),
    updated_at              timestamptz,
    PRIMARY KEY (product_manufacturer_id, language_id)
);

CREATE TABLE product (
    id              uuid PRIMARY KEY,
    parent_id       uuid REFERENCES product (id) ON DELETE CASCADE,
    product_number  text        NOT NULL UNIQUE,
    active          boolean,
    stock           integer,
    tax_id          uuid REFERENCES tax (id),
    manufacturer_id uuid REFERENCES product_manufacturer (id) ON DELETE SET NULL,
    price           jsonb,
    ean             text,
    manufacturer_number text,
    weight          numeric,
    width           numeric,
    height          numeric,
    length          numeric,
    min_purchase    integer,
    max_purchase    integer,
    purchase_steps  integer,
    is_closeout     boolean,
    shipping_free   boolean,
    mark_as_topseller boolean,
    release_date    timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz
);
CREATE INDEX product_parent_idx ON product (parent_id);
CREATE INDEX product_manufacturer_idx ON product (manufacturer_id);

CREATE TABLE product_translation (
    product_id       uuid        NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    language_id      uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name             text,
    description      text,
    keywords         text,
    meta_title       text,
    meta_description text,
    custom_fields    jsonb,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz,
    PRIMARY KEY (product_id, language_id)
);

CREATE TABLE product_category (
    product_id  uuid NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    category_id uuid NOT NULL REFERENCES category (id) ON DELETE CASCADE,
    PRIMARY KEY (product_id, category_id)
);
CREATE INDEX product_category_category_idx ON product_category (category_id);

-- Administration users, integrations and ACL --------------------------------

CREATE TABLE admin_user (
    id          uuid PRIMARY KEY,
    username    text        NOT NULL UNIQUE,
    first_name  text        NOT NULL DEFAULT '',
    last_name   text        NOT NULL DEFAULT '',
    email       text        NOT NULL UNIQUE,
    password    text        NOT NULL,
    admin       boolean     NOT NULL DEFAULT false,
    active      boolean     NOT NULL DEFAULT true,
    locale_code text        NOT NULL DEFAULT 'en-GB',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz
);

CREATE TABLE integration (
    id                uuid PRIMARY KEY,
    label             text        NOT NULL,
    access_key        text        NOT NULL UNIQUE,
    secret_access_key text        NOT NULL,
    admin             boolean     NOT NULL DEFAULT false,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz
);

CREATE TABLE acl_role (
    id          uuid PRIMARY KEY,
    name        text        NOT NULL,
    description text,
    privileges  jsonb       NOT NULL DEFAULT '[]',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz
);

CREATE TABLE acl_user_role (
    user_id     uuid NOT NULL REFERENCES admin_user (id) ON DELETE CASCADE,
    acl_role_id uuid NOT NULL REFERENCES acl_role (id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, acl_role_id)
);

CREATE TABLE refresh_token (
    token_hash text PRIMARY KEY,
    user_id    uuid        NOT NULL REFERENCES admin_user (id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE audit_log (
    id         uuid PRIMARY KEY,
    entity     text        NOT NULL,
    entity_id  uuid        NOT NULL,
    action     text        NOT NULL,
    payload    jsonb       NOT NULL,
    actor_id   uuid,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_entity_idx ON audit_log (entity, entity_id);

-- Default data (ids identical to Shopware's Defaults) -------------------------

INSERT INTO language (id, name, locale_code) VALUES
    ('2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'English', 'en-GB'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Deutsch', 'de-DE');

INSERT INTO currency (id, iso_code, factor, symbol, decimal_precision, position) VALUES
    ('b7d2554b0ce847cd82f3ac9bd1c0dfca', 'EUR', 1, '€', 2, 1);
INSERT INTO currency_translation (currency_id, language_id, short_name, name) VALUES
    ('b7d2554b0ce847cd82f3ac9bd1c0dfca', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'EUR', 'Euro'),
    ('b7d2554b0ce847cd82f3ac9bd1c0dfca', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'EUR', 'Euro');

INSERT INTO tax (id, tax_rate, name, position) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6b01', 19, 'Standard rate', 1),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6b02', 7, 'Reduced rate', 2);

INSERT INTO category (id, position, type) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6c01', 0, 'page');
INSERT INTO category_translation (category_id, language_id, name) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6c01', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Home'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6c01', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Startseite');

INSERT INTO sales_channel (id, type_id, access_key, language_id, currency_id, navigation_category_id) VALUES
    ('98432def39fc4624b33213a56b8c944d', '8a243080f92e4c719546314b577cf82b', 'SWSCFERROSHOPDEFAULT00000',
     '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'b7d2554b0ce847cd82f3ac9bd1c0dfca', '0191a6f0f4a07c7e9b1c2d3e4f5a6c01');
INSERT INTO sales_channel_translation (sales_channel_id, language_id, name) VALUES
    ('98432def39fc4624b33213a56b8c944d', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Storefront'),
    ('98432def39fc4624b33213a56b8c944d', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Storefront');

INSERT INTO acl_role (id, name, description, privileges) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a6d01', 'Administrator', 'Full access', '["*"]');
