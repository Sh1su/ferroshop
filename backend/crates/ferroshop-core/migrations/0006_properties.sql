-- Product properties: groups (colour, size) with options (red, M).
-- `product_property` describes a product; `product_option` defines what makes a variant a variant.

CREATE TABLE property_group (
    id                             uuid PRIMARY KEY,
    display_type                   text        NOT NULL DEFAULT 'text'
        CHECK (display_type IN ('text', 'color', 'media')),
    sorting_type                   text        NOT NULL DEFAULT 'alphanumeric'
        CHECK (sorting_type IN ('alphanumeric', 'position')),
    position                       integer     NOT NULL DEFAULT 0,
    filterable                     boolean     NOT NULL DEFAULT true,
    visible_on_product_detail_page boolean     NOT NULL DEFAULT true,
    created_at                     timestamptz NOT NULL DEFAULT now(),
    updated_at                     timestamptz
);

CREATE TABLE property_group_translation (
    property_group_id uuid        NOT NULL REFERENCES property_group (id) ON DELETE CASCADE,
    language_id       uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name              text,
    description       text,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz,
    PRIMARY KEY (property_group_id, language_id)
);

CREATE TABLE property_group_option (
    id                uuid PRIMARY KEY,
    group_id          uuid        NOT NULL REFERENCES property_group (id) ON DELETE CASCADE,
    position          integer     NOT NULL DEFAULT 0,
    color_hex_code    text,
    media_id          uuid REFERENCES media (id) ON DELETE SET NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz
);
CREATE INDEX property_group_option_group_idx ON property_group_option (group_id);

CREATE TABLE property_group_option_translation (
    property_group_option_id uuid        NOT NULL REFERENCES property_group_option (id) ON DELETE CASCADE,
    language_id              uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name                     text,
    created_at               timestamptz NOT NULL DEFAULT now(),
    updated_at               timestamptz,
    PRIMARY KEY (property_group_option_id, language_id)
);

CREATE TABLE product_property (
    product_id uuid NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    option_id  uuid NOT NULL REFERENCES property_group_option (id) ON DELETE CASCADE,
    PRIMARY KEY (product_id, option_id)
);
CREATE INDEX product_property_option_idx ON product_property (option_id);

CREATE TABLE product_option (
    product_id uuid NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    option_id  uuid NOT NULL REFERENCES property_group_option (id) ON DELETE RESTRICT,
    PRIMARY KEY (product_id, option_id)
);
CREATE INDEX product_option_option_idx ON product_option (option_id);
