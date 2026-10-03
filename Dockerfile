FROM rust:1.88-bookworm AS builder

ARG TARGETARCH

WORKDIR /rwth.cool

# Map Docker's TARGETARCH to a Rust musl target triple so the same Dockerfile
# builds for every platform passed to `docker buildx build --platform`.
RUN case "$TARGETARCH" in \
      amd64) echo x86_64-unknown-linux-musl > /rust-target ;; \
      arm64) echo aarch64-unknown-linux-musl > /rust-target ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && rustup target add "$(cat /rust-target)"

# Compile the dependencies first so that they are cached across source changes.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked --target "$(cat /rust-target)" \
    && rm -rf src

# The templates and static assets are embedded into the binary at compile time.
COPY src src
COPY templates templates
COPY static static
RUN cargo build --release --locked --target "$(cat /rust-target)" \
    && cp "target/$(cat /rust-target)/release/rwth_cool" /rwth_cool

FROM scratch

WORKDIR /rwth.cool
COPY --from=builder /rwth_cool .
COPY redirects.toml .

USER 10001:10001

EXPOSE 3000

CMD ["./rwth_cool"]
