FROM rust:1.88-bookworm AS builder

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /rwth.cool

# Compile the dependencies first so that they are cached across source changes.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked --target x86_64-unknown-linux-musl \
    && rm -rf src

# The templates and static assets are embedded into the binary at compile time.
COPY src src
COPY templates templates
COPY static static
RUN cargo build --release --locked --target x86_64-unknown-linux-musl

FROM scratch

WORKDIR /rwth.cool
COPY --from=builder /rwth.cool/target/x86_64-unknown-linux-musl/release/rwth_cool .
COPY redirects.toml .

USER 10001:10001

EXPOSE 3000

CMD ["./rwth_cool"]
