-- Shopping experiences: pages made of sections, blocks and slots (Shopware's CMS model).
-- A slot's `config` is translated: `{"content": {"source": "static", "value": "<p>Hi</p>"}}`.

CREATE TABLE cms_page (
    id         uuid PRIMARY KEY,
    type       text        NOT NULL DEFAULT 'landingpage'
        CHECK (type IN ('landingpage', 'product_list', 'product_detail', 'page')),
    css_class  text,
    locked     boolean     NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);

CREATE TABLE cms_page_translation (
    cms_page_id uuid        NOT NULL REFERENCES cms_page (id) ON DELETE CASCADE,
    language_id uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    name        text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz,
    PRIMARY KEY (cms_page_id, language_id)
);

CREATE TABLE cms_section (
    id                  uuid PRIMARY KEY,
    page_id             uuid        NOT NULL REFERENCES cms_page (id) ON DELETE CASCADE,
    position            integer     NOT NULL DEFAULT 0,
    type                text        NOT NULL DEFAULT 'default' CHECK (type IN ('default', 'sidebar')),
    sizing_mode         text        NOT NULL DEFAULT 'boxed',
    mobile_behavior     text        NOT NULL DEFAULT 'wrap',
    background_color    text,
    background_media_id uuid REFERENCES media (id) ON DELETE SET NULL,
    css_class           text,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz
);
CREATE INDEX cms_section_page_idx ON cms_section (page_id);

CREATE TABLE cms_block (
    id               uuid PRIMARY KEY,
    section_id       uuid        NOT NULL REFERENCES cms_section (id) ON DELETE CASCADE,
    position         integer     NOT NULL DEFAULT 0,
    section_position text        NOT NULL DEFAULT 'main' CHECK (section_position IN ('main', 'sidebar')),
    type             text        NOT NULL,
    name             text,
    margin_top       text,
    margin_bottom    text,
    margin_left      text,
    margin_right     text,
    background_color text,
    css_class        text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz
);
CREATE INDEX cms_block_section_idx ON cms_block (section_id);

CREATE TABLE cms_slot (
    id         uuid PRIMARY KEY,
    block_id   uuid        NOT NULL REFERENCES cms_block (id) ON DELETE CASCADE,
    type       text        NOT NULL,
    slot       text        NOT NULL,
    locked     boolean     NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);
CREATE INDEX cms_slot_block_idx ON cms_slot (block_id);

CREATE TABLE cms_slot_translation (
    cms_slot_id uuid        NOT NULL REFERENCES cms_slot (id) ON DELETE CASCADE,
    language_id uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    config      jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz,
    PRIMARY KEY (cms_slot_id, language_id)
);

ALTER TABLE category ADD COLUMN cms_page_id uuid REFERENCES cms_page (id) ON DELETE SET NULL;

-- The default listing page: filters in the sidebar, the product listing in the main column.
-- Categories without a page of their own use it.
INSERT INTO cms_page (id, type, locked) VALUES ('0191a6f0f4a07c7e9b1c2d3e4f5a8001', 'product_list', true);
INSERT INTO cms_page_translation (cms_page_id, language_id, name) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8001', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Default listing page'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8001', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Standard-Listingseite');
INSERT INTO cms_section (id, page_id, position, type) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8101', '0191a6f0f4a07c7e9b1c2d3e4f5a8001', 0, 'sidebar');
INSERT INTO cms_block (id, section_id, position, section_position, type, margin_top, margin_bottom, margin_left, margin_right) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8201', '0191a6f0f4a07c7e9b1c2d3e4f5a8101', 0, 'sidebar', 'sidebar-filter', '0', '0', '0', '0'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8202', '0191a6f0f4a07c7e9b1c2d3e4f5a8101', 1, 'main', 'product-listing', '20px', '20px', '0', '0');
INSERT INTO cms_slot (id, block_id, type, slot, locked) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8301', '0191a6f0f4a07c7e9b1c2d3e4f5a8201', 'sidebar-filter', 'content', true),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8302', '0191a6f0f4a07c7e9b1c2d3e4f5a8202', 'product-listing', 'content', true);
INSERT INTO cms_slot_translation (cms_slot_id, language_id, config) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8301', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', '{}'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a8302', '2fbb5fe2e29a4d70aa5854ce7ce3e20b',
     '{"boxLayout": {"source": "static", "value": "standard"}, "showSorting": {"source": "static", "value": true}, "useCustomSorting": {"source": "static", "value": false}}');
