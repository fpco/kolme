CREATE TABLE combined_stream(
    id INTEGER PRIMARY KEY NOT NULL,
    height INTEGER NOT NULL,
    added TIMESTAMP NOT NULL,
    is_execution BOOLEAN NOT NULL,
    rendered TEXT NOT NULL,
    UNIQUE (height, is_execution)
);

-- Unified table for state, just for code simplicity
CREATE TABLE state_payload(
    hash BLOB PRIMARY KEY NOT NULL,
    payload TEXT NOT NULL
);

CREATE TABLE event_stream(
    height INTEGER PRIMARY KEY NOT NULL,
    hash BLOB NOT NULL UNIQUE,
    state BLOB NOT NULL REFERENCES state_payload(hash),
    rendered_id INTEGER NOT NULL REFERENCES combined_stream(id)
);

CREATE TABLE execution_stream(
    height INTEGER PRIMARY KEY NOT NULL REFERENCES event_stream(height),
    framework_state BLOB NOT NULL REFERENCES state_payload(hash),
    app_state BLOB NOT NULL REFERENCES state_payload(hash),
    rendered_id INTEGER NOT NULL REFERENCES combined_stream(id)
);

CREATE TABLE execution_logs(
    height INTEGER NOT NULL REFERENCES execution_stream(height),
    message INTEGER NOT NULL,
    position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(height, message, position)
);

CREATE TABLE execution_loads(
    height INTEGER NOT NULL REFERENCES execution_stream(height),
    message INTEGER NOT NULL,
    position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(height, message, position)
);

CREATE TABLE execution_actions(
    id INTEGER PRIMARY KEY NOT NULL,
    height INTEGER NOT NULL REFERENCES execution_stream(height),
    message INTEGER NOT NULL,
    position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    UNIQUE(height, message, position)
);

CREATE TABLE listener_bridge_events(
    chain TEXT NOT NULL,
    bridge_event_id INTEGER NOT NULL,
    public_key BLOB NOT NULL,
    signature BLOB NOT NULL,
    height INTEGER REFERENCES event_stream(height),
    UNIQUE(chain, bridge_event_id, public_key)
);
