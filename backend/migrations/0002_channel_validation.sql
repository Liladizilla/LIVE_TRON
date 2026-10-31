-- Tighten channel identifiers and categories without deleting or rewriting data.
ALTER TABLE channels
    DROP CONSTRAINT IF EXISTS channels_slug_format;

ALTER TABLE channels
    ADD CONSTRAINT channels_slug_format
    CHECK (
        length(slug) BETWEEN 3 AND 48
        AND slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'
    );

ALTER TABLE channels
    ADD CONSTRAINT channels_category_valid
    CHECK (category IN ('gaming', 'music', 'talk', 'education', 'sports', 'irl', 'commerce'));
