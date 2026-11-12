# Design note

## Architecture

The server is a binary (`src/bin/iconator-server.rs`) in the same package as the library. The lookup tables are already compiled into the library, so a separate crate or a workspace would add ceremony without protecting any boundary.

- **axum 0.8 on tokio**, with **tower-http** for request tracing, a 5 s timeout and a 16 KiB body limit. Axum is the default choice on the tokio/tower stack: typed extractors, `Json` and `IntoResponse` keep the handlers small.
- A two-variant `ApiError` enum implementing `IntoResponse` maps every failure to a JSON body; **anyhow** is used only in `main` (invalid `PORT`, bind failure). With two variants and no error chaining, `thiserror` would add a dependency without adding clarity.
- Configuration through `HOST` (default `0.0.0.0`), `PORT` (default `8080`) and `RUST_LOG` (default `info`).

## API

| Request | Response |
| --- | --- |
| `GET /icons/file?path=src/main.rs` | `200 {"icon_id":525}` |
| `GET /icons/folder?path=.github` | `200 {"icon_id":862}` |
| no icon for that name | `404 {"error":"not_found"}` |
| missing, empty, > 4096 bytes, NUL byte, no file name (`/`, `.`, `..`) | `400 {"error":"invalid_path","message":"..."}` |
| `GET /health` | `200 {"status":"ok"}` |

## Tradeoffs

- **Query parameter, not a path segment**: inputs are file paths and contain slashes; `?path=` avoids catch-all routes and encoding ambiguities.
- **404 rather than `200 {"icon_id": null}`**: a miss is visible from the status code alone; the client falls back to its default icon either way.
- **IDs only, no SVG**: this matches the library's contract. Clients (IDEs) are expected to ship or cache the icon set; serving `icons/*.svg` would be a separate static-file concern, ideally on a CDN.
- **No filesystem access**: the library only looks at the last path component and the extension, so `../../etc/passwd` is just a lookup of `passwd`. Path traversal is not a risk; the server still bounds the input size.
- **Library fix**: `get_icon_for_file` / `get_icon_for_folder` used `unwrap()` on `file_name()` and `extension()`, which panicked on `""`, `/`, `..` or an unknown name without extension such as `foo`. They now return `None` (two `?`), with regression tests. Lookup rules are unchanged. I kept `lazy_static` (clippy pedantic suggests `LazyLock`) to keep the diff focused.
- **Unix-only**, because the library already relies on `OsStrExt`.
- Tests call the router with `tower::ServiceExt::oneshot`, so they need no open port.

## Production

- **Packaging**: a multi-stage Docker image, built with `cargo build --release --locked` in `rust:1-slim`, then the binary copied into `gcr.io/distroless/cc-debian12:nonroot` (small image, no shell, non-root user). Roughly:

  ```dockerfile
  FROM rust:1-slim AS build
  WORKDIR /app
  COPY . .
  RUN cargo build --release --locked --bin iconator-server

  FROM gcr.io/distroless/cc-debian12:nonroot
  COPY --from=build /app/target/release/iconator-server /iconator-server
  ENV PORT=8080
  EXPOSE 8080
  ENTRYPOINT ["/iconator-server"]
  ```
- **Running**: the service is stateless and lookups are in memory, so it scales horizontally behind any load balancer (Cloud Run, Fly.io, Kubernetes). `/health` serves liveness/readiness probes, and the process shuts down gracefully on `SIGTERM`, finishing in-flight requests.
- **Caching**: a given path always maps to the same icon for a given build, so successful responses carry `Cache-Control: public, max-age=86400` and can sit behind a CDN.
- **Observability**: structured logs through `tracing` and `TraceLayer`. Next steps: JSON logs, request IDs, Prometheus metrics (latency, status codes, hit/miss ratio); rate limiting and TLS at the proxy.
- **CI**: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo audit`, then the image build.
