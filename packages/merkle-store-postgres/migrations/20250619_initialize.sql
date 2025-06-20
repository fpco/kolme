CREATE TABLE merkle_contents(
    hash     BYTEA PRIMARY KEY,
    payload  BYTEA,
    children BYTEA[]
);
