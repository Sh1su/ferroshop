-- Salutations for customers and addresses, and stock bookkeeping for orders.

CREATE TABLE salutation (
    id             uuid PRIMARY KEY,
    salutation_key text        NOT NULL UNIQUE,
    position       integer     NOT NULL DEFAULT 0,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz
);

CREATE TABLE salutation_translation (
    salutation_id uuid        NOT NULL REFERENCES salutation (id) ON DELETE CASCADE,
    language_id   uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    display_name  text,
    letter_name   text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz,
    PRIMARY KEY (salutation_id, language_id)
);

INSERT INTO salutation (id, salutation_key, position) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9001', 'mr', 1),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9002', 'mrs', 2),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9003', 'not_specified', 3);
INSERT INTO salutation_translation (salutation_id, language_id, display_name, letter_name) VALUES
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9001', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Mr.', 'Dear Mr.'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9002', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Mrs.', 'Dear Mrs.'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9003', '2fbb5fe2e29a4d70aa5854ce7ce3e20b', 'Not specified', ' '),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9001', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Herr', 'Sehr geehrter Herr'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9002', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Frau', 'Sehr geehrte Frau'),
    ('0191a6f0f4a07c7e9b1c2d3e4f5a9003', '0191a6f0f4a07c7e9b1c2d3e4f5a6b7c', 'Keine Angabe', ' ');

ALTER TABLE customer ADD COLUMN salutation_id uuid REFERENCES salutation (id) ON DELETE SET NULL;
ALTER TABLE customer_address ADD COLUMN salutation_id uuid REFERENCES salutation (id) ON DELETE SET NULL;
ALTER TABLE order_address ADD COLUMN salutation_id uuid REFERENCES salutation (id) ON DELETE SET NULL;

-- `booked`: the order took its products' stock (checkout); `released`: cancelling gave it back.
-- Orders created otherwise (`none`) never touch stock.
ALTER TABLE sales_order ADD COLUMN stock_state text NOT NULL DEFAULT 'none'
    CHECK (stock_state IN ('none', 'booked', 'released'));
