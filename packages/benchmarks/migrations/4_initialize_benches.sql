CREATE TABLE bench_merkle_contents(
    hash     BYTEA PRIMARY KEY,
    payload  BYTEA,
    children BYTEA[]
);

CREATE TYPE bench_children AS (
    bytes   BYTEA[]
);
