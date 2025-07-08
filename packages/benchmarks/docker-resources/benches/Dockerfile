FROM rust:1.84.0-bookworm AS builder

RUN apt update &&   \
    apt install -y  \
        libudev-dev \
        libssl-dev  \
        mold
WORKDIR /kolme
COPY . .
COPY .env.dockerfile .env
WORKDIR /kolme/packages/integration-tests
RUN cargo build --bench insertion --release
RUN cp $(find /kolme/target/release/deps -maxdepth 1 -type f -executable -name 'insertion-*') /kolme/bench


FROM debian:bookworm
WORKDIR /kolme
COPY --from=builder /kolme/bench .
CMD ["/kolme/bench", "--bench"]
