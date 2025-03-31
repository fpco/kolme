#!/usr/bin/env bash

set -euxo pipefail

### mdbook

MDBOOK_VER="v0.4.31"
MDBOOK_KATEX_VER="v0.5.3"
MDBOOK_TOC_VER="0.12.0"
MDBOOK_MERMAID_VER="0.13.0"

mkdir -p docs/bin
cd docs/bin

wget "https://github.com/rust-lang/mdBook/releases/download/${MDBOOK_VER}/mdbook-${MDBOOK_VER}-x86_64-unknown-linux-musl.tar.gz"
tar xf "mdbook-${MDBOOK_VER}-x86_64-unknown-linux-musl.tar.gz"

wget "https://github.com/lzanini/mdbook-katex/releases/download/${MDBOOK_KATEX_VER}/mdbook-katex-${MDBOOK_KATEX_VER}-x86_64-unknown-linux-musl.tar.gz"
tar xf "mdbook-katex-${MDBOOK_KATEX_VER}-x86_64-unknown-linux-musl.tar.gz"

wget "https://github.com/badboy/mdbook-toc/releases/download/${MDBOOK_TOC_VER}/mdbook-toc-${MDBOOK_TOC_VER}-x86_64-unknown-linux-musl.tar.gz"
tar xf "mdbook-toc-${MDBOOK_TOC_VER}-x86_64-unknown-linux-musl.tar.gz"

wget "https://download.fpcomplete.com/bin/mdbook-mermaid-${MDBOOK_MERMAID_VER}-static.gz"
gunzip mdbook-mermaid-${MDBOOK_MERMAID_VER}-static.gz
mv mdbook-mermaid-${MDBOOK_MERMAID_VER}-static mdbook-mermaid
chmod +x mdbook-mermaid

export PATH="$(pwd):$PATH"

cd ..
rm -rf ../mdbook
mkdir ../mdbook
mdbook build -d ../mdbook
cd ..
