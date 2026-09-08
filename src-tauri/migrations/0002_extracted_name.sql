-- The name an item's extracted file is given on disk.
--
-- An item that exists only as clipboard data — a screenshot, most of the time —
-- has no file until the user asks to open it, reveal it or drag it out. One is
-- written for them then, under a name derived from the item.
--
-- The name has to be decided once and kept, not derived afresh every time. It
-- was derived afresh, and the folder was wiped on every launch, so nothing ever
-- noticed. Once the files started outliving the process, the same picture
-- opened on three different days became `screenshot.png`, `screenshot-1.png`
-- and `screenshot-2.png`, because each run found the earlier name taken and
-- stepped around it. Recording the name here fixes both directions at once: one
-- item can only ever produce the one file, and the name it holds can never be
-- handed to a different item, even after the file itself has been deleted or
-- has expired.
ALTER TABLE items ADD COLUMN extracted_name TEXT;

-- The uniqueness is the whole point, and it is enforced here rather than in
-- application code so that two items racing for the same name cannot both win.
-- Partial, because every item that has never been extracted holds NULL and
-- those must not collide with each other.
CREATE UNIQUE INDEX IF NOT EXISTS idx_items_extracted_name
    ON items(extracted_name) WHERE extracted_name IS NOT NULL;
