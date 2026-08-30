-- CripCode Community Templates — initial schema.
--
-- Minimal production metadata table. No marketplace features (payments,
-- ratings, reviews, moderation, subscriptions) by design.
CREATE TABLE IF NOT EXISTS templates (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    description   TEXT NOT NULL,
    author        TEXT NOT NULL,
    category      TEXT NOT NULL,
    framework     TEXT NOT NULL,
    thumbnail_key TEXT,
    version       TEXT NOT NULL,
    object_key    TEXT NOT NULL UNIQUE,
    object_size   BIGINT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS templates_category_idx ON templates (category);
CREATE INDEX IF NOT EXISTS templates_framework_idx ON templates (framework);
CREATE INDEX IF NOT EXISTS templates_created_at_idx ON templates (created_at);
