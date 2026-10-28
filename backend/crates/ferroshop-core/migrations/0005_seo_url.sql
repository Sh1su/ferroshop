-- SEO URLs per sales channel and language. `foreign_key` points at a category or product,
-- depending on `route_name`. Old canonical URLs stay as non-canonical rows, so links keep working.

CREATE TABLE seo_url (
    id               uuid PRIMARY KEY,
    sales_channel_id uuid        NOT NULL REFERENCES sales_channel (id) ON DELETE CASCADE,
    language_id      uuid        NOT NULL REFERENCES language (id) ON DELETE CASCADE,
    foreign_key      uuid        NOT NULL,
    route_name       text        NOT NULL,
    path_info        text        NOT NULL,
    seo_path_info    text        NOT NULL,
    is_canonical     boolean     NOT NULL DEFAULT false,
    -- Set by hand in the Administration: generation leaves the URL alone.
    is_modified      boolean     NOT NULL DEFAULT false,
    is_deleted       boolean     NOT NULL DEFAULT false,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz,
    UNIQUE (sales_channel_id, language_id, seo_path_info)
);
CREATE INDEX seo_url_foreign_key_idx ON seo_url (foreign_key);
CREATE UNIQUE INDEX seo_url_canonical_idx ON seo_url (sales_channel_id, language_id, foreign_key, route_name)
    WHERE is_canonical;
