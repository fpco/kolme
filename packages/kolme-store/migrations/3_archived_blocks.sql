CREATE TABLE archived_blocks (
    height      bigint primary key,
    archived_at timestamp not null
);

CREATE MATERIALIZED VIEW latest_archived_block_height AS (
    SELECT height
    FROM archived_blocks
    ORDER BY height DESC
    LIMIT 1
);
