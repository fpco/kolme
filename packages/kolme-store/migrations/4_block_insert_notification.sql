CREATE OR REPLACE FUNCTION notify_kolme_blocks_insert()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('kolme_blocks_insert', NEW.height::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS notify_kolme_blocks_insert ON blocks;
CREATE TRIGGER notify_kolme_blocks_insert
AFTER INSERT ON blocks
FOR EACH ROW
EXECUTE FUNCTION notify_kolme_blocks_insert();

