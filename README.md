# RWTH.cool😎

Source code of [RWTH.cool](https://rwth.cool).
The site provides a collection of useful links for students at RWTH Aachen University, inspired by [TUM.sexy](https://tum.sexy).

Visit the site for a list of available redirects.

To follow a redirect, for example the mensa menu, either visit the corresponding subdomain (`mensa.rwth.cool`) or route (`rwth.cool/mensa`).

## Contributing

You are welcome to open issues and/or pull requests if you are missing features.

To add redirects, edit the `redirects.toml` file. `category` and `aliases` are optional:

```toml
[redirects.mensa]
url = "https://www.studierendenwerk-aachen.de/de/Gastronomie/mensa-academica-wochenplan.html"
description = "Check today's menu at Mensa Academica"
category = "Academic"
aliases = ["hunger", "essen", "academica"]
```

The configuration is validated when the application starts: unknown or
misspelled fields, duplicate aliases, aliases that collide with another
shortcut, invalid URLs and an invalid `BASE_URL` all abort the startup with an
error message. `cargo test` runs the same validation against the shipped
`redirects.toml`, so mistakes are caught in CI before deployment.

## Development

The Rust toolchain is pinned in `rust-toolchain.toml` and installed
automatically by `rustup` on first use.

```sh
cargo run          # serves http://localhost:3000
cargo test         # unit, configuration and HTTP integration tests
cargo fmt --check  # formatting
cargo clippy --all-targets --all-features -- -D warnings
```

With the default `BASE_URL`, generated links and redirects point at
`https://rwth.cool`. Set `BASE_URL=http://localhost:3000` to make them point at
your local instance.

Subdomain shortcuts can be tested locally via `localhost`:

```sh
echo '127.0.0.1 mensa.localhost' | sudo tee -a /etc/hosts
curl -H 'Host: mensa.localhost' http://127.0.0.1:3000/
```

### Configuration

| Environment variable | Default             | Description                                                     |
| -------------------- | ------------------- | --------------------------------------------------------------- |
| `RUST_LOG`           | `info`              | Log filter, e.g. `debug` for per-request logs                    |
| `PORT`               | `3000`              | Port to listen on                                               |
| `BASE_URL`           | `https://rwth.cool` | Public base URL used for `robots.txt`, `sitemap.xml` and links  |
| `REDIRECTS_FILE`     | `redirects.toml`    | Path to the redirects configuration                             |

### Endpoints

| Path             | Description                                            |
| ---------------- | ------------------------------------------------------ |
| `/` and `/{key}` | Shortcut resolution (also by subdomain)                 |
| `/favicon.png`   | Favicon                                                 |
| `/healthz`       | Liveness probe (also used by the Docker health check)   |
| `/robots.txt`    | `robots.txt` with the configured sitemap URL            |
| `/sitemap.xml`   | Sitemap with the configured base URL                    |
| `/search.js`     | Client-side search helper                               |

## Docker

The multi-stage `Dockerfile` builds a static musl binary for a `scratch` image.
Templates and static assets are embedded into the binary; only `redirects.toml`
is read at runtime. The container runs as an unprivileged user and includes a
`--healthcheck` mode used by Docker's health check.

The published image is a multi-architecture manifest for `linux/amd64` and
`linux/arm64`; `TARGETARCH` selects the matching Rust musl target inside the
build, so the same Dockerfile works on both.

```sh
# Build for the host architecture
docker build -t rwth.cool .
docker run --rm -p 3000:3000 rwth.cool

# Build and push a multi-architecture manifest
docker buildx build --platform linux/amd64,linux/arm64 -t rwth.cool --push .
```

## License

This project is licensed under the MIT license
