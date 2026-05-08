-- Add migration script here

-- 1. Create the Notes table
CREATE TABLE
    notes (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL UNIQUE,
        content TEXT NOT NULL,
        metadata TEXT NOT NULL DEFAULT '{}',
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

-- 2. Create the Links table (the Graph edges)
CREATE TABLE
    links (
        from_note_id INTEGER NOT NULL,
        to_note_id INTEGER NOT NULL,
        -- another link_types possible: backlink, tag, summary, duplicate
        link_type TEXT NOT NULL CHECK (link_type IN ('reference', 'related', 'parent', 'child')),
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        PRIMARY KEY (from_note_id, to_note_id),
        FOREIGN KEY (from_note_id) REFERENCES notes (id) ON DELETE CASCADE,
        FOREIGN KEY (to_note_id) REFERENCES notes (id) ON DELETE CASCADE
    );

CREATE INDEX idx_links_to_node ON links (to_note_id);
CREATE INDEX idx_links_from_node ON links (from_note_id);