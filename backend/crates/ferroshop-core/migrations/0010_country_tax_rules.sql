-- Optional country overrides for the product tax classes. When no row matches,
-- Store API pricing keeps using the tax class's base rate.
CREATE TABLE tax_rule (
    id         uuid PRIMARY KEY,
    tax_id     uuid        NOT NULL REFERENCES tax (id) ON DELETE CASCADE,
    country_id uuid        NOT NULL REFERENCES country (id) ON DELETE CASCADE,
    tax_rate   numeric     NOT NULL CHECK (tax_rate >= 0 AND tax_rate <= 100),
    priority   integer     NOT NULL DEFAULT 1 CHECK (priority >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz
);

CREATE INDEX tax_rule_tax_country_priority_idx ON tax_rule (tax_id, country_id, priority DESC);

-- German demo defaults, matching the seeded standard and reduced tax classes.
INSERT INTO tax_rule (id, tax_id, country_id, tax_rate, priority) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7301', '0191a6f0f4a07c7e9b1c2d3e4f5a6b01', '0191a6f0f4a07c7e9b1c2d3e4f5a7001', 19, 1),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a7302', '0191a6f0f4a07c7e9b1c2d3e4f5a6b02', '0191a6f0f4a07c7e9b1c2d3e4f5a7001', 7, 1);
