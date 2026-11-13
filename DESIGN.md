# Design note

## Architecture

One package: the library, plus the server as a binary in `src/bin/iconator-server.rs`. The
tables are compiled into the library, so a second crate would add ceremony without protecting
any boundary.

- **axum 0.8 on tokio**, the default on the tokio/tower stack: typed extractors, `Json` and
  `IntoResponse` keep each handler to a few lines, and tower-http's `TraceLayer` logs requests.
- **Errors**: an `ApiError` enum turns every failure into a JSON body, including the query
  strings axum itself rejects. `anyhow` only appears in `main` (bad `PORT`, bind failure).
- **Tests** go through the router with `tower::ServiceExt::oneshot`, no open port needed.

## Library fixes

The server is only as right as the lookups, so I fixed the library first.

- **Matching.** The table comes from vscode-icons, and VS Code lowercases
  [names](https://github.com/microsoft/vscode/blob/223c263c8bf610f045e3aad647c8eb3fc4774272/src/vs/editor/common/services/getIconClasses.ts)
  and [theme keys](https://github.com/microsoft/vscode/blob/223c263c8bf610f045e3aad647c8eb3fc4774272/src/vs/workbench/services/themes/browser/fileIconThemeData.ts),
  then tries every dotted suffix, longest first (see the README). The library compared raw
  bytes and used `Path::extension()`, so `Cargo.toml` got the TOML icon (630, not 85),
  `README.MD` got nothing and `index.d.ts` got TypeScript (633, not 618). 76 of the 711
  extension keys contain a dot and could never match.
- **Robustness.** No more panics on `""`, `/`, `..` or `foo`; `expect` instead of
  `unsafe { unwrap_unchecked() }` (it runs once per map); the build script reruns when the
  table changes and fails if a key points to two icons.
- **Portability.** `LazyLock` replaces `lazy_static`, and `as_encoded_bytes()` replaces the
  Unix-only `OsStrExt`, so it also builds on Windows and macOS.

## Tradeoffs

- **`?path=`**, not a path segment: inputs contain slashes, and a query avoids catch-all routes.
- **404 for a miss**, not `200 {"icon_id": null}`: the client shows its default icon either way.
- **IDs only**, as the library returns. The SVGs are a static-file job for a CDN.
- **No filesystem access**: `../../etc/passwd` is a lookup of `passwd`. Input is capped at 4096 bytes.
- **No body limit or timeout in the app**: GET only, answered from memory. Those belong to the proxy.
- **Cacheable**: a build always gives the same answer, so hits carry `max-age=86400`.
- **Left out**: a batch endpoint. An IDE opening 500 files sends 500 requests; I would add
  `POST /icons` once traffic shows it matters. Clients should send `/` separators, since a
  Linux server reads `C:\src\Cargo.toml` as one name.

## Production

- **Image**: the `Dockerfile` builds on `rust:1-slim-bookworm` and runs on
  `gcr.io/distroless/cc-debian12:nonroot` (no shell, non-root). Same Debian release on both
  sides, so the binary never needs a newer glibc than the runtime has.
- **Running**: stateless and in memory, so it scales horizontally (Cloud Run, Fly.io,
  Kubernetes). `/health` serves the probes, and `SIGTERM` drains in-flight requests. TLS, rate
  limits and timeouts sit at the proxy; a CDN can cache the responses.
- **Observability**: `tracing` logs today. Next: JSON logs, request IDs, Prometheus metrics for
  latency, status codes and hit/miss ratio, which shows the names the table is missing.
- **CI** (`.github/workflows/ci.yml`): fmt, clippy `-D warnings`, tests, image build. Next:
  `cargo deny` for advisories and licences.
