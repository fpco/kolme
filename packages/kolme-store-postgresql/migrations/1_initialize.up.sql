CREATE TABLE blocks(
    height SERIAL PRIMARY KEY,
    rendered TEXT NOT NULL,
    blockhash TEXT NOT NULL,
    txhash TEXT NOT NULL
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
