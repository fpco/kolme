#!/usr/bin/env bash

#
# Building a custom image of cowasm/optimizer with rust 1.84.0
#

if docker image inspect optimizer:1.84 > /dev/null 2>&1; then
    echo "Optimizer image already built"
    exit 0
fi

echo "Building optimizer image with rust v1.84"

WORK_DIR=`mktemp -d -p /tmp`

git clone https://github.com/CosmWasm/optimizer $WORK_DIR
cd $WORK_DIR
git checkout 4dedb39736d34f27cd799f222a30a4f1b230fb12 # v0.16.1

sed -i 's/1\.81/1.84/g' Dockerfile
docker build -t optimizer:1.84 .
