CREATE TABLE event_stream(
    height INTEGER PRIMARY KEY NOT NULL,
    added TIMESTAMP NOT NULL,
    payload BLOB NOT NULL,
    signature BLOB NOT NULL,
    rendered BLOB NOT NULL
);

-- Unified table for framework and app state, just for code simplicity
CREATE TABLE state_payload(
    hash BLOB PRIMARY KEY NOT NULL,
    payload BLOB NOT NULL
);

CREATE TABLE state_stream(
    height INTEGER PRIMARY KEY NOT NULL REFERENCES event_stream(height),
    added TIMESTAMP NOT NULL,
    framework_state_hash BLOB NOT NULL REFERENCES state_payload(hash),
    app_state_hash BLOB NOT NULL REFERENCES state_payload(hash),
    rendered BLOB NOT NULL
);

CREATE TABLE state_logs(
    height INTEGER NOT NULL REFERENCES state_stream(height),
    position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(height, position)
);

CREATE TABLE state_loads(
    height INTEGER NOT NULL REFERENCES state_stream(height),
    position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(height, position)
);

CREATE TABLE actions(
    id INTEGER PRIMARY KEY NOT NULL,
    height INTEGER NOT NULL REFERENCES state_stream(height),
    payload TEXT NOT NULL
);
