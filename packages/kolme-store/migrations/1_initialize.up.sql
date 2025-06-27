CREATE TABLE blocks(
    height SERIAL8 PRIMARY KEY,
    blockhash BYTEA NOT NULL UNIQUE,
    rendered TEXT NOT NULL,
    txhash BYTEA NOT NULL UNIQUE,
    framework_state_hash BYTEA NOT NULL,
    app_state_hash BYTEA NOT NULL,
    logs_hash BYTEA NOT NULL
);
