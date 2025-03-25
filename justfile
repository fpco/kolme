test:
    cargo sqlx database reset -y --source kolme/migrations
    cargo sqlx migrate run --source kolme/migrations
    cargo sqlx prepare --workspace
    cargo test
