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
    framework_state_hash BLOB NOT NULL,
    app_state_hash BLOB NOT NULL
);

CREATE TABLE messages(
    id INTEGER PRIMARY KEY NOT NULL,
    height INTEGER NOT NULL REFERENCES blocks(height),
    message INTEGER NOT NULL,
    UNIQUE(height, message)
);

CREATE TABLE logs(
    message INTEGER NOT NULL REFERENCES messages(id),
    position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(message, position)
);
