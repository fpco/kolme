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

CREATE OR REPLACE FUNCTION notify_new_block()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('new_block_channel', NEW::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER new_block_trigger
AFTER INSERT ON blocks
FOR EACH ROW
EXECUTE FUNCTION notify_new_block();
