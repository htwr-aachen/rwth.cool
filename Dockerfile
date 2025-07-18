FROM rust:1.88-bullseye

COPY . /rwth.cool
WORKDIR /rwth.cool

RUN cargo build --release

EXPOSE 3000

CMD ["./target/release/rwth_cool"]
