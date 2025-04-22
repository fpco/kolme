FROM debian

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends libssl-dev ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY example-p2p /usr/local/bin/

ENTRYPOINT ["/usr/local/bin/example-p2p"]
