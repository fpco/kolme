CREATE TABLE merkle_contents(
    hash     BYTEA PRIMARY KEY,
    payload  BYTEA,
    children BYTEA[]
);

CREATE TYPE children AS (
    bytes   BYTEA[]
);
