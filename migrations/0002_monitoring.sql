-- A schedule generation distinguishes wall-clock reanchors from duplicate slots.
ALTER TABLE monitor_state ADD COLUMN epoch_id INTEGER REFERENCES schedule_epochs(id);
ALTER TABLE monitor_state ADD COLUMN next_due_ms INTEGER;
ALTER TABLE monitor_state ADD COLUMN cadence TEXT NOT NULL DEFAULT 'normal' CHECK(cadence IN ('normal','confirmation'));
ALTER TABLE monitor_state ADD COLUMN first_failure_ms INTEGER;
ALTER TABLE monitor_state ADD COLUMN last_result_json TEXT CHECK(length(last_result_json)<=512);
ALTER TABLE monitor_state ADD COLUMN system_error TEXT CHECK(length(system_error)<=64);
-- WP-01/02 did not run a monitoring scheduler. Preserve any preconfigured
-- definitions, starting coverage now instead of inventing past enabled slots.
INSERT INTO schedule_epochs(monitor_id,revision,anchor_at_ms,interval_seconds,enabled_start_ms,enabled_end_ms,discontinuity)
SELECT id,revision,CAST(unixepoch('subsec')*1000 AS INTEGER),interval_seconds,
       CAST(unixepoch('subsec')*1000 AS INTEGER),
       CASE WHEN enabled=0 THEN CAST(unixepoch('subsec')*1000 AS INTEGER) END,'restart'
FROM monitors;
INSERT INTO monitor_state(monitor_id,status,failure_streak)
SELECT id,CASE WHEN enabled=1 THEN 'unknown' ELSE 'paused' END,0 FROM monitors WHERE id NOT IN (SELECT monitor_id FROM monitor_state);
UPDATE monitor_state SET epoch_id=(SELECT max(id) FROM schedule_epochs WHERE monitor_id=monitor_state.monitor_id),
next_due_ms=(SELECT anchor_at_ms FROM schedule_epochs WHERE id=(SELECT max(id) FROM schedule_epochs WHERE monitor_id=monitor_state.monitor_id));
CREATE TABLE results_v2 (
    monitor_id INTEGER NOT NULL REFERENCES monitors(id),
    revision INTEGER NOT NULL CHECK(revision>=1),
    -- Zero is reserved for results imported from the unused foundation schema.
    epoch_id INTEGER NOT NULL DEFAULT 0 CHECK(epoch_id>=0),
    scheduled_slot_ms INTEGER NOT NULL,
    observed_at_ms INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK(outcome IN ('success','failure','unknown')),
    elapsed_ms INTEGER CHECK(elapsed_ms BETWEEN 0 AND 30000),
    status_code INTEGER CHECK(status_code BETWEEN 100 AND 599),
    error_category TEXT CHECK(length(error_category)<=64),
    destination_family TEXT CHECK(destination_family IN ('ipv4','ipv6')),
    PRIMARY KEY(monitor_id,epoch_id,revision,scheduled_slot_ms)
) STRICT;
INSERT INTO results_v2(monitor_id,revision,scheduled_slot_ms,observed_at_ms,outcome,elapsed_ms,status_code,error_category,destination_family)
SELECT monitor_id,revision,scheduled_slot_ms,observed_at_ms,outcome,elapsed_ms,status_code,error_category,destination_family FROM probe_results;
DROP TABLE probe_results;
ALTER TABLE results_v2 RENAME TO probe_results;
CREATE INDEX results_monitor_time ON probe_results(monitor_id,scheduled_slot_ms);
CREATE INDEX results_retention ON probe_results(scheduled_slot_ms);
CREATE INDEX incidents_retention ON incidents(ended_at_ms);
CREATE INDEX stats_retention ON hourly_stats(hour_ms);
-- Constant-size persisted counters avoid count(*) scans on every sample.
CREATE TABLE monitoring_counts (
    id INTEGER PRIMARY KEY CHECK(id=1),
    raw_rows INTEGER NOT NULL CHECK(raw_rows BETWEEN 0 AND 1010000),
    -- 2160 full hours plus a partial boundary hour for each of 50 monitors.
    hourly_rows INTEGER NOT NULL CHECK(hourly_rows BETWEEN 0 AND 108050),
    epoch_rows INTEGER NOT NULL CHECK(epoch_rows BETWEEN 0 AND 10000)
) STRICT;
INSERT INTO monitoring_counts VALUES(1,(SELECT count(*) FROM probe_results),(SELECT count(*) FROM hourly_stats),(SELECT count(*) FROM schedule_epochs));
CREATE TRIGGER raw_insert AFTER INSERT ON probe_results BEGIN UPDATE monitoring_counts SET raw_rows=raw_rows+1 WHERE id=1; END;
CREATE TRIGGER raw_delete AFTER DELETE ON probe_results BEGIN UPDATE monitoring_counts SET raw_rows=raw_rows-1 WHERE id=1; END;
CREATE TRIGGER stats_insert AFTER INSERT ON hourly_stats BEGIN UPDATE monitoring_counts SET hourly_rows=hourly_rows+1 WHERE id=1; END;
CREATE TRIGGER stats_delete AFTER DELETE ON hourly_stats BEGIN UPDATE monitoring_counts SET hourly_rows=hourly_rows-1 WHERE id=1; END;
CREATE TRIGGER epochs_insert AFTER INSERT ON schedule_epochs BEGIN UPDATE monitoring_counts SET epoch_rows=epoch_rows+1 WHERE id=1; END;
CREATE TRIGGER epochs_delete AFTER DELETE ON schedule_epochs BEGIN UPDATE monitoring_counts SET epoch_rows=epoch_rows-1 WHERE id=1; END;
