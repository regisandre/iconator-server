# syntax=docker/dockerfile:1

# Build and runtime both use Debian 12 (bookworm): a binary linked against a newer glibc than
# the runtime image ships fails at startup.
FROM rust:1-slim-bookworm AS build
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked --bin iconator-server \
    && cp target/release/iconator-server /iconator-server

# No shell, no package manager, runs as an unprivileged user.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /iconator-server /iconator-server
EXPOSE 8080
ENTRYPOINT ["/iconator-server"]
