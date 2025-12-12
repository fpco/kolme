CREATE OR REPLACE FUNCTION notify_insert_blocks()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('insert_blocks', json_build_object('height',NEW.height::text)::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER notify_insert_blocks
AFTER INSERT ON blocks
FOR EACH ROW
EXECUTE FUNCTION notify_insert_blocks();

