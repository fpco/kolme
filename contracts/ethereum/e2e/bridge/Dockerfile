FROM ubuntu:24.04 AS foundry

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl ca-certificates git jq && \
    rm -rf /var/lib/apt/lists/*

SHELL ["/bin/bash", "-lc"]

ARG FOUNDRY_VERSION=v1.3.1

RUN curl -L https://foundry.paradigm.xyz | bash && \
    /root/.foundry/bin/foundryup --install "${FOUNDRY_VERSION}"

# Install a fixed solc binary so `forge build` does not depend on external solc mirrors.
RUN curl -L https://github.com/ethereum/solidity/releases/download/v0.8.30/solc-static-linux \
    -o /usr/local/bin/solc && \
    chmod +x /usr/local/bin/solc

ENV PATH="/root/.foundry/bin:${PATH}"

FROM foundry AS contract_builder

WORKDIR /app

ARG ANVIL_MNEMONIC="test test test test test test test test test test test junk"

COPY foundry.toml foundry.lock ./
COPY lib ./lib
COPY src ./src

# Build contract artifacts inside the image.
RUN forge build --use /usr/local/bin/solc

FROM foundry

WORKDIR /app

ARG ANVIL_MNEMONIC="test test test test test test test test test test test junk"

RUN mkdir -p /bootstrap && \
    printf "%s" "${ANVIL_MNEMONIC}" > /bootstrap/mnemonic.txt

COPY --from=contract_builder /app/out /app/out
COPY e2e/bridge/entrypoint.sh /entrypoint.sh

RUN chmod +x /entrypoint.sh

EXPOSE 8545

ENTRYPOINT ["/entrypoint.sh"]
