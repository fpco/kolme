FROM osmolabs/osmosis:25.2.0-alpine

RUN apk add dasel

RUN mkdir -p /var/workspace
VOLUME ["/var/workspace"]

WORKDIR /osmosis

ADD --chmod=755 ./setup.sh /osmosis/setup.sh

ENTRYPOINT [ "/osmosis/setup.sh" ]
