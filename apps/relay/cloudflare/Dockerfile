# The relay as a Cloudflare Container: the same binary as everywhere else, run
# under Litestream, which brings the database back from R2 on start and streams
# every change to R2 within a second. Built by Workers Builds from the
# repository root (`image_build_context` in wrangler.jsonc); runs anywhere
# Docker does, given the R2 variables in entrypoint.sh.

FROM rust:1-bookworm AS build
# .cargo/config.toml runs clippy on every build.
RUN rustup component add clippy
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p tackly-relay

FROM debian:bookworm-slim
ARG TARGETARCH
ARG LITESTREAM_VERSION=0.5.17
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) arch=x86_64; sum=cfb371176d164437ae869f8351cfde49bd1804ae71c61923f75c9cba9c9c006d ;; \
      arm64) arch=arm64;  sum=f8ca4a050095c1efbda2c4365172e61bf9d955ea0d9ac42f448b52e51819baa5 ;; \
      *) echo "no Litestream build for $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    file="litestream-${LITESTREAM_VERSION}-linux-${arch}.tar.gz"; \
    curl -sSfL -o "/tmp/$file" "https://github.com/benbjohnson/litestream/releases/download/v${LITESTREAM_VERSION}/$file"; \
    echo "$sum  /tmp/$file" | sha256sum -c -; \
    tar -xzf "/tmp/$file" -C /usr/local/bin litestream; \
    rm "/tmp/$file"
COPY --from=build /src/target/release/tackly-relay /usr/local/bin/tackly-relay
COPY apps/relay/cloudflare/litestream.yml /etc/litestream.yml
COPY apps/relay/cloudflare/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN mkdir /data
ENV DATABASE_URL=sqlite:///data/tackly-relay.db \
    TACKLY_BIND=0.0.0.0:3000
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
