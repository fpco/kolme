-- Merkle hash storage
CREATE TABLE merkle_hashes(
    hash BYTEA PRIMARY KEY NOT NULL,
    payload BYTEA NOT NULL
);

CREATE TABLE blocks(
    height SERIAL8 PRIMARY KEY,
    blockhash BYTEA NOT NULL UNIQUE,
    rendered TEXT NOT NULL,
    txhash BYTEA NOT NULL UNIQUE,
    framework_state_hash BYTEA NOT NULL REFERENCES merkle_hashes(hash),
    app_state_hash BYTEA NOT NULL REFERENCES merkle_hashes(hash),
    logs_hash BYTEA NOT NULL REFERENCES merkle_hashes(hash)
);
