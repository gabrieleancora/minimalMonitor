-- UTC timestamps are integer Unix milliseconds. No credentials or monitors are
-- seeded. Feature workers and their transactional mutations belong to WP-02–04.
CREATE TABLE schema_versions (
    version INTEGER PRIMARY KEY CHECK(version > 0),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
    applied_at_ms INTEGER NOT NULL CHECK(applied_at_ms >= 0)
) STRICT;

CREATE TABLE admin (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    username TEXT NOT NULL UNIQUE CHECK(length(username) BETWEEN 1 AND 64),
    password_hash TEXT NOT NULL CHECK(length(password_hash) BETWEEN 1 AND 1024),
    credential_generation INTEGER NOT NULL CHECK(credential_generation >= 1),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY CHECK(length(token_hash) = 32),
    admin_id INTEGER NOT NULL REFERENCES admin(id),
    csrf_binding BLOB NOT NULL CHECK(length(csrf_binding) = 32),
    created_at_ms INTEGER NOT NULL,
    last_used_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL CHECK(expires_at_ms > created_at_ms)
) STRICT;
CREATE INDEX sessions_expiry ON sessions(expires_at_ms);
CREATE TRIGGER sessions_bound BEFORE INSERT ON sessions WHEN (SELECT count(*) FROM sessions) >= 5
BEGIN SELECT RAISE(ABORT, 'session_capacity'); END;

CREATE TABLE monitors (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
    kind TEXT NOT NULL CHECK(kind IN ('http','tcp','ping')),
    target TEXT NOT NULL CHECK(length(CAST(target AS BLOB)) BETWEEN 1 AND 2048),
    port INTEGER CHECK(port BETWEEN 1 AND 65535),
    accepted_statuses TEXT CHECK(length(accepted_statuses) <= 1024),
    interval_seconds INTEGER NOT NULL CHECK(interval_seconds BETWEEN 30 AND 3600),
    deadline_seconds INTEGER NOT NULL CHECK(deadline_seconds BETWEEN 1 AND 30 AND deadline_seconds < interval_seconds),
    failure_threshold INTEGER NOT NULL CHECK(failure_threshold BETWEEN 1 AND 5),
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    alerts INTEGER NOT NULL CHECK(alerts IN (0,1)),
    address_family TEXT NOT NULL CHECK(address_family IN ('auto','ipv4','ipv6')),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    tombstoned_at_ms INTEGER,
    CHECK((kind = 'tcp' AND port IS NOT NULL) OR (kind != 'tcp' AND port IS NULL))
) STRICT;
CREATE TRIGGER monitors_bound BEFORE INSERT ON monitors WHEN (SELECT count(*) FROM monitors) >= 50
BEGIN SELECT RAISE(ABORT, 'monitor_capacity'); END;

CREATE TABLE schedule_epochs (
    id INTEGER PRIMARY KEY,
    monitor_id INTEGER NOT NULL REFERENCES monitors(id),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    anchor_at_ms INTEGER NOT NULL,
    interval_seconds INTEGER NOT NULL CHECK(interval_seconds BETWEEN 30 AND 3600),
    enabled_start_ms INTEGER NOT NULL,
    enabled_end_ms INTEGER CHECK(enabled_end_ms >= enabled_start_ms),
    discontinuity TEXT CHECK(discontinuity IN ('restart','clock_jump','configuration_changed','pause','resume')),
    aggregated_through_ms INTEGER
) STRICT;
CREATE INDEX epochs_monitor_time ON schedule_epochs(monitor_id, enabled_start_ms);
CREATE TABLE incidents (
    id INTEGER PRIMARY KEY,
    monitor_id INTEGER NOT NULL REFERENCES monitors(id),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    first_failure_at_ms INTEGER NOT NULL,
    confirmed_at_ms INTEGER NOT NULL CHECK(confirmed_at_ms >= first_failure_at_ms),
    ended_at_ms INTEGER CHECK(ended_at_ms >= confirmed_at_ms),
    end_reason TEXT CHECK(end_reason IN ('recovered','paused','configuration_changed','deleted')),
    last_evidence_at_ms INTEGER NOT NULL,
    observation_gap INTEGER NOT NULL DEFAULT 0 CHECK(observation_gap IN (0,1)),
    CHECK((ended_at_ms IS NULL AND end_reason IS NULL) OR (ended_at_ms IS NOT NULL AND end_reason IS NOT NULL))
) STRICT;
CREATE INDEX incidents_monitor_time ON incidents(monitor_id, confirmed_at_ms);
CREATE UNIQUE INDEX incidents_one_open ON incidents(monitor_id) WHERE ended_at_ms IS NULL;
-- No closed-incident row ceiling may shorten the confirmed 180-day retention.
-- WP-03 must resolve bounded overflow before implementing incident writes.
CREATE TABLE monitor_state (
    monitor_id INTEGER PRIMARY KEY REFERENCES monitors(id),
    status TEXT NOT NULL CHECK(status IN ('up','pending_failure','down','unknown','paused')),
    failure_streak INTEGER NOT NULL CHECK(failure_streak BETWEEN 0 AND 5),
    last_due_at_ms INTEGER,
    last_completed_at_ms INTEGER,
    last_outcome TEXT CHECK(last_outcome IN ('success','failure','unknown')),
    last_error_category TEXT CHECK(length(last_error_category) <= 64),
    active_incident_id INTEGER REFERENCES incidents(id)
) STRICT;
CREATE TABLE probe_results (
    monitor_id INTEGER NOT NULL REFERENCES monitors(id),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    scheduled_slot_ms INTEGER NOT NULL,
    observed_at_ms INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK(outcome IN ('success','failure','unknown')),
    elapsed_ms INTEGER CHECK(elapsed_ms BETWEEN 0 AND 30000),
    status_code INTEGER CHECK(status_code BETWEEN 100 AND 599),
    error_category TEXT CHECK(length(error_category) <= 64),
    destination_family TEXT CHECK(destination_family IN ('ipv4','ipv6')),
    PRIMARY KEY(monitor_id, revision, scheduled_slot_ms)
) STRICT;
CREATE INDEX results_monitor_time ON probe_results(monitor_id, observed_at_ms);
CREATE TABLE hourly_stats (
    monitor_id INTEGER NOT NULL REFERENCES monitors(id),
    hour_ms INTEGER NOT NULL,
    expected INTEGER NOT NULL CHECK(expected >= 0),
    successful INTEGER NOT NULL CHECK(successful >= 0),
    failed INTEGER NOT NULL CHECK(failed >= 0),
    unknown INTEGER NOT NULL CHECK(unknown >= 0),
    latency_sum_ms INTEGER NOT NULL CHECK(latency_sum_ms >= 0),
    latency_count INTEGER NOT NULL CHECK(latency_count >= 0),
    latency_max_ms INTEGER CHECK(latency_max_ms BETWEEN 0 AND 30000),
    PRIMARY KEY(monitor_id, hour_ms),
    CHECK(expected = successful + failed + unknown),
    CHECK(latency_count <= successful + failed)
) STRICT;

CREATE TABLE notification_outbox (
    id INTEGER PRIMARY KEY,
    event_identity TEXT NOT NULL UNIQUE CHECK(length(event_identity) BETWEEN 1 AND 100),
    incident_id INTEGER REFERENCES incidents(id),
    kind TEXT NOT NULL CHECK(kind IN ('down','recovery','test','suppression')),
    destination_revision INTEGER NOT NULL CHECK(destination_revision >= 1),
    payload TEXT NOT NULL CHECK(length(CAST(payload AS BLOB)) <= 16384),
    state TEXT NOT NULL CHECK(state IN ('pending','sending','delivered','failed','expired','canceled')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 10),
    next_attempt_at_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL CHECK(expires_at_ms > created_at_ms),
    message_id INTEGER,
    UNIQUE(incident_id, kind)
) STRICT;
CREATE INDEX outbox_due ON notification_outbox(state, next_attempt_at_ms);
CREATE TABLE settings (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    public_origin TEXT CHECK(length(public_origin) <= 2048),
    telegram_chat_id INTEGER,
    telegram_enabled INTEGER NOT NULL DEFAULT 0 CHECK(telegram_enabled IN (0,1)),
    destination_revision INTEGER NOT NULL DEFAULT 1 CHECK(destination_revision >= 1)
) STRICT;
CREATE TABLE request_keys (
    admin_id INTEGER NOT NULL REFERENCES admin(id),
    key TEXT NOT NULL CHECK(length(key) BETWEEN 1 AND 128),
    operation TEXT NOT NULL CHECK(length(operation) BETWEEN 1 AND 64),
    request_digest BLOB NOT NULL CHECK(length(request_digest) = 32),
    result_reference TEXT NOT NULL CHECK(length(result_reference) <= 256),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY(admin_id, key)
) STRICT;
CREATE TABLE health_state (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    dropped_alerts INTEGER NOT NULL DEFAULT 0 CHECK(dropped_alerts >= 0),
    persistence_faults INTEGER NOT NULL DEFAULT 0 CHECK(persistence_faults >= 0),
    aggregation_horizon_ms INTEGER,
    updated_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE audit_events (
    id INTEGER PRIMARY KEY,
    created_at_ms INTEGER NOT NULL,
    category TEXT NOT NULL CHECK(length(category) BETWEEN 1 AND 64),
    entity_id INTEGER,
    outcome TEXT NOT NULL CHECK(length(outcome) BETWEEN 1 AND 64)
) STRICT;
INSERT INTO settings(id, revision) VALUES (1, 1);
INSERT INTO health_state(id, updated_at_ms) VALUES (1, 0);
