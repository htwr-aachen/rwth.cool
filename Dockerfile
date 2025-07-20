FROM rust:1.88-bullseye as builder

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /rwth.cool
COPY . .

RUN cargo build --release --target x86_64-unknown-linux-musl

FROM scratch

WORKDIR /rwth.cool
COPY templates templates
COPY redirects.toml .
COPY --from=builder /rwth.cool/target/x86_64-unknown-linux-musl/release/rwth_cool .

EXPOSE 3000

CMD ["./rwth_cool"]
