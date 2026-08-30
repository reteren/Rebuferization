-- Rebuffer store schema, v1
-- SQLite 3.38+ with FTS5. Apply once at first run; later changes go in migrations/.

-- ---------------------------------------------------------------------------
-- items: one row per clipboard entry
-- ---------------------------------------------------------------------------
CREATE TABLE items (
    id            INTEGER PRIMARY KEY,

    kind          TEXT    NOT NULL,          -- 'text' | 'image' | 'video' | 'file' | 'other'
    sub_kind      TEXT,                      -- 'plain' | 'rich' | 'code' | 'link' | 'color' | 'animated'

    hash          TEXT    NOT NULL,          -- BLAKE3 of the primary blob / normalized text
    blob_path     TEXT,                      -- relative to blobs/, NULL for references
    thumb_path    TEXT,                      -- relative to blobs/thumbs/

    is_reference  INTEGER NOT NULL DEFAULT 0,-- 1 = manual shelf item, path only, never expires
    ref_path      TEXT,                      -- source path for references

    title         TEXT,                      -- user-set display name, overrides derived name
    preview_text  TEXT,                      -- first ~200 chars, also the FTS source
    ext           TEXT,                      -- 'PNG', 'TXT', 'MP4' — shown on the card
    mime          TEXT,

    byte_size     INTEGER NOT NULL DEFAULT 0,
    width         INTEGER,                   -- images/video only
    height        INTEGER,
    duration_ms   INTEGER,                   -- video only

    source_app    TEXT,                      -- process name at capture time
    copy_count    INTEGER NOT NULL DEFAULT 1,

    created_at    INTEGER NOT NULL,          -- unix ms; bumped on duplicate re-copy
    first_seen_at INTEGER NOT NULL,          -- unix ms; never changes
    last_used_at  INTEGER,

    pinned        INTEGER NOT NULL DEFAULT 0
);

-- Duplicate detection. References are excluded so the same file can sit on the
-- shelf and also appear as a normal capture.
CREATE UNIQUE INDEX idx_items_hash ON items(hash) WHERE is_reference = 0;

CREATE INDEX idx_items_created  ON items(created_at DESC);
CREATE INDEX idx_items_kind     ON items(kind, created_at DESC);
CREATE INDEX idx_items_pinned   ON items(pinned, created_at DESC);
CREATE INDEX idx_items_ext      ON items(ext);
CREATE INDEX idx_items_size     ON items(byte_size DESC);
-- The janitor's hot query: oldest, unpinned, not a reference.
CREATE INDEX idx_items_janitor  ON items(created_at) WHERE pinned = 0 AND is_reference = 0;

-- ---------------------------------------------------------------------------
-- item_formats: extra clipboard formats so paste can restore faithfully
-- ---------------------------------------------------------------------------
CREATE TABLE item_formats (
    id          INTEGER PRIMARY KEY,
    item_id     INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    format      TEXT    NOT NULL,            -- 'CF_UNICODETEXT', 'HTML Format', 'Rich Text Format', ...
    blob_path   TEXT,                        -- large payloads on disk
    inline_data BLOB,                        -- payloads under 64 KB stored directly
    byte_size   INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_formats_item ON item_formats(item_id);

-- ---------------------------------------------------------------------------
-- item_files: individual paths from a multi-file CF_HDROP capture
-- ---------------------------------------------------------------------------
CREATE TABLE item_files (
    id        INTEGER PRIMARY KEY,
    item_id   INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    path      TEXT    NOT NULL,
    file_name TEXT    NOT NULL,
    byte_size INTEGER,
    position  INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_files_item ON item_files(item_id);

-- ---------------------------------------------------------------------------
-- Full-text search over preview_text and title
-- ---------------------------------------------------------------------------
CREATE VIRTUAL TABLE items_fts USING fts5(
    preview_text,
    title,
    content     = 'items',
    content_rowid = 'id',
    tokenize    = "unicode61 remove_diacritics 2"
);

CREATE TRIGGER items_fts_ai AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, preview_text, title)
    VALUES (new.id, new.preview_text, new.title);
END;

CREATE TRIGGER items_fts_ad AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, preview_text, title)
    VALUES ('delete', old.id, old.preview_text, old.title);
END;

CREATE TRIGGER items_fts_au AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, preview_text, title)
    VALUES ('delete', old.id, old.preview_text, old.title);
    INSERT INTO items_fts(rowid, preview_text, title)
    VALUES (new.id, new.preview_text, new.title);
END;

-- ---------------------------------------------------------------------------
-- meta: schema version and bookkeeping
-- ---------------------------------------------------------------------------
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO meta(key, value) VALUES
    ('schema_version', '1'),
    ('created_at',     CAST(strftime('%s','now') AS TEXT) || '000');


-- ===========================================================================
-- Reference queries
-- ===========================================================================

-- Newest page, All tab
--   SELECT * FROM items ORDER BY created_at DESC LIMIT 200 OFFSET ?;

-- Images tab
--   SELECT * FROM items WHERE kind = 'image' ORDER BY created_at DESC LIMIT 200;

-- Search
--   SELECT i.* FROM items_fts f
--   JOIN items i ON i.id = f.rowid
--   WHERE items_fts MATCH ?
--   ORDER BY rank LIMIT 200;

-- Extension facets for the filter dropdown
--   SELECT ext, COUNT(*) AS n FROM items
--   WHERE ext IS NOT NULL GROUP BY ext ORDER BY n DESC;

-- Storage stats by type
--   SELECT kind, COUNT(*) AS n, SUM(byte_size) AS bytes FROM items GROUP BY kind;

-- Janitor: expired by age
--   SELECT id, hash, blob_path, thumb_path FROM items
--   WHERE pinned = 0 AND is_reference = 0 AND created_at < ?;

-- Janitor: oldest first, for the size cap
--   SELECT id, hash, blob_path, thumb_path, byte_size FROM items
--   WHERE pinned = 0 AND is_reference = 0 ORDER BY created_at ASC;

-- Blob refcount before deleting a file from disk
--   SELECT COUNT(*) FROM items WHERE hash = ?;

-- Duplicate bump
--   UPDATE items SET created_at = ?, copy_count = copy_count + 1 WHERE hash = ?;
