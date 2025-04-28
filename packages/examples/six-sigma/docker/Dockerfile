FROM debian

RUN apt-get update -qq && \
    apt-get install -y --no-install-recommends libssl-dev ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY example-six-sigma /usr/local/bin/

ENTRYPOINT ["/usr/local/bin/example-six-sigma"]
