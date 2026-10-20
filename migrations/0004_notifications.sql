-- Telegram state is bounded and durable; no bot token is stored in SQLite.
ALTER TABLE incidents ADD COLUMN telegram_down_delivered INTEGER NOT NULL DEFAULT 0
    CHECK(telegram_down_delivered IN (0,1));
UPDATE incidents SET telegram_down_delivered=1 WHERE EXISTS (
    SELECT 1 FROM notification_outbox o WHERE o.incident_id=incidents.id
    AND o.kind='down' AND o.state='delivered');
ALTER TABLE notification_outbox ADD COLUMN terminal_at_ms INTEGER;
ALTER TABLE notification_outbox ADD COLUMN error_category TEXT
    CHECK(error_category IN ('transient','rate_limited','configuration','interrupted'));
UPDATE notification_outbox SET terminal_at_ms=created_at_ms
    WHERE state IN ('delivered','failed','expired','canceled');
CREATE TABLE telegram_state (
    id INTEGER PRIMARY KEY CHECK(id=1),
    bot_fingerprint BLOB CHECK(bot_fingerprint IS NULL OR length(bot_fingerprint)=32),
    cooldown_until_ms INTEGER NOT NULL DEFAULT 0 CHECK(cooldown_until_ms>=0),
    paced_until_ms INTEGER NOT NULL DEFAULT 0 CHECK(paced_until_ms>=0),
    test_after_ms INTEGER NOT NULL DEFAULT 0 CHECK(test_after_ms>=0),
    configuration_failed INTEGER NOT NULL DEFAULT 0 CHECK(configuration_failed IN (0,1)),
    last_error TEXT CHECK(last_error IN ('transient','rate_limited','configuration','interrupted')),
    last_success_ms INTEGER,
    request_sequence INTEGER NOT NULL DEFAULT 0 CHECK(request_sequence>=0),
    acknowledged_sequence INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged_sequence>=0 AND acknowledged_sequence<=request_sequence),
    request_destination_revision INTEGER NOT NULL DEFAULT 1 CHECK(request_destination_revision>=1),
    summarized_drops INTEGER NOT NULL DEFAULT 0 CHECK(summarized_drops>=0)
) STRICT;
INSERT INTO telegram_state(id) VALUES(1);
CREATE INDEX outbox_terminal ON notification_outbox(terminal_at_ms)
    WHERE state IN ('delivered','failed','expired','canceled');
CREATE TRIGGER outbox_pending_bound BEFORE INSERT ON notification_outbox
WHEN NEW.state IN ('pending','sending') AND (
    (SELECT count(*) FROM notification_outbox WHERE state IN ('pending','sending'))>=1000
    OR coalesce((SELECT sum(length(CAST(payload AS BLOB))) FROM notification_outbox
        WHERE state IN ('pending','sending')),0)+length(CAST(NEW.payload AS BLOB))>2097152)
BEGIN SELECT RAISE(ABORT,'outbox_capacity'); END;
-- Each transition prunes at most one row, so even bulk cancellation stays bounded.
CREATE TRIGGER outbox_terminal_bound AFTER UPDATE OF state ON notification_outbox
WHEN NEW.state IN ('delivered','failed','expired','canceled')
BEGIN
    DELETE FROM notification_outbox WHERE id IN (
        SELECT id FROM notification_outbox WHERE state IN ('delivered','failed','expired','canceled')
        ORDER BY coalesce(terminal_at_ms,created_at_ms),id LIMIT 1 OFFSET 0
    ) AND (SELECT count(*) FROM notification_outbox WHERE state IN ('delivered','failed','expired','canceled'))>2000;
END;
