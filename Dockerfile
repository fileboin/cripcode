# CripCode Template API — production container (Coolify + Traefik).
# Build context: repository root. The API is a self-contained Rust binary;
# the ship-studio lib links tauri's GTK/WebKit stack, so the build stage needs
# those headers and the runtime stage their shared libraries — the API itself
# never touches them.

FROM rust:1.88-slim-bookworm AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config file \
        libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY src-tauri ./src-tauri
# tauri_build::build() (build.rs) validates frontendDist ("../dist") even though
# the API build never touches frontend assets — an empty directory satisfies it.
RUN mkdir -p dist

WORKDIR /build/src-tauri
RUN cargo build --release --bin cripcode-template-api --features template-postgres

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl \
        libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 libxdo3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /build/src-tauri/target/release/cripcode-template-api /usr/local/bin/cripcode-template-api

RUN groupadd --gid 1000 cripcode \
    && useradd --system --uid 1000 --gid 1000 cripcode \
    && mkdir -p /var/lib/cripcode-templates/objects \
    && chown -R cripcode:cripcode /var/lib/cripcode-templates

# Container defaults; every value can be overridden via Coolify env variables.
ENV CRIPCODE_TEMPLATE_API_BIND=0.0.0.0:8787 \
    CRIPCODE_TEMPLATE_API_DATA_DIR=/var/lib/cripcode-templates \
    CRIPCODE_TEMPLATES_STORAGE_PROVIDER=local
VOLUME ["/var/lib/cripcode-templates"]

EXPOSE 8787
USER 1000:1000
ENTRYPOINT ["/usr/local/bin/cripcode-template-api"]
CMD ["serve"]
