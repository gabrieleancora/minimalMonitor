-- Before this upgrade the service allocated only positive SQLite rowids.
-- Keep those identities intact and reserve the negative range for new monitors:
-- deleted legacy IDs cannot be reconstructed from a live max(id), but no future
-- service allocation can collide with them. One durable decreasing sequence
-- prevents reuse in the new range after delete, cleanup, crash or restart.
CREATE TABLE monitor_identity_allocator (
    id INTEGER PRIMARY KEY CHECK(id=1),
    last_id INTEGER NOT NULL CHECK(last_id<=0)
) STRICT;
INSERT INTO monitor_identity_allocator(id,last_id)
SELECT 1,min(0,
    coalesce((SELECT min(id) FROM monitors),0),
    coalesce((SELECT min(CAST(result_reference AS INTEGER)) FROM request_keys WHERE operation='monitor_create'),0));
