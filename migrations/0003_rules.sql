-- A reader's filters on a feed, judged by Jev for each new article.
CREATE TABLE feed_rules (
    id INTEGER PRIMARY KEY,
    feed_id INTEGER NOT NULL REFERENCES feeds (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    condition TEXT NOT NULL CHECK (trim(condition) <> ''),
    action TEXT NOT NULL CHECK (action IN ('hide', 'keep_only'))
);

CREATE INDEX feed_rules_by_feed ON feed_rules (feed_id, position);

-- Articles a rule kept out of the list. Remembering them means the next fetch neither stores
-- them nor asks Jev about them again.
CREATE TABLE filtered_items (
    feed_id INTEGER NOT NULL REFERENCES feeds (id) ON DELETE CASCADE,
    guid TEXT NOT NULL,
    PRIMARY KEY (feed_id, guid)
) WITHOUT ROWID;
