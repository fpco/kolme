-- Merkle hash storage
CREATE TABLE merkle_hashes(
    hash BLOB PRIMARY KEY NOT NULL,
    payload BLOB NOT NULL
);

CREATE TABLE blocks(
    height INTEGER PRIMARY KEY NOT NULL,
    blockhash BLOB NOT NULL UNIQUE,
    -- The fully signed block
    rendered TEXT NOT NULL,
    txhash BLOB NOT NULL UNIQUE,
    framework_state_hash BLOB NOT NULL REFERENCES merkle_hashes(hash),
    app_state_hash BLOB NOT NULL REFERENCES merkle_hashes(hash),
    logs_hash BLOB NOT NULL REFERENCES merkle_hashes(hash)
);
