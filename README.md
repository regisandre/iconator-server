# iconator

A file and folder icon lookup library, as an IDE would use for its project tree, and an HTTP
server that exposes it. Give it a path, get back the ID of an SVG in `./icons`.

The design note, with the tradeoffs and how I would run this in production, is in
[DESIGN.md](DESIGN.md). The original assignment is at the end of this file.

## Run

```sh
cargo run --release --bin iconator-server        # listens on 0.0.0.0:8080
HOST=127.0.0.1 PORT=3000 RUST_LOG=debug cargo run --bin iconator-server
```

Or with Docker:

```sh
docker build -t iconator .
docker run --rm -p 8080:8080 iconator
```

## API

```sh
curl 'http://localhost:8080/icons/file?path=src/main.rs'      # 200 {"icon_id":525}
curl 'http://localhost:8080/icons/file?path=Cargo.toml'       # 200 {"icon_id":85}
curl 'http://localhost:8080/icons/folder?path=.github'        # 200 {"icon_id":862}
curl 'http://localhost:8080/icons/file?path=foo'              # 404 {"error":"not_found"}
curl 'http://localhost:8080/icons/file?path=..'               # 400 {"error":"invalid_path","message":"..."}
curl 'http://localhost:8080/icons/file?path=a&path=b'         # 400 {"error":"invalid_query","message":"..."}
curl 'http://localhost:8080/health'                           # 200 {"status":"ok"}
```

| Route | Success | Errors |
| --- | --- | --- |
| `GET /icons/file?path=<path>` | `200 {"icon_id": u64}`, `Cache-Control: public, max-age=86400` | `400 invalid_path`, `400 invalid_query`, `404 not_found` |
| `GET /icons/folder?path=<path>` | same | same |
| `GET /health` | `200 {"status":"ok"}` | |

`invalid_path` covers a missing or empty `path`, more than 4096 bytes, a NUL byte, and a path
with no last component (`/`, `.`, `..`). `invalid_query` is a query string that does not parse,
such as `path` given twice. Every error body is JSON.

## How names are matched

The icon associations come from [vscode-icons](https://github.com/microsoft/vscode-icons), so
the library matches names the way VS Code does:

- only the last path component counts, and nothing is read from disk;
- matching is case-insensitive (`Cargo.toml`, `README.MD`, `.GitHub`);
- a file is matched by its full name first, then by every dotted suffix from the longest:
  `index.d.ts` tries `index.d.ts`, `d.ts`, then `ts`, and `.env` tries `env`.

`build.rs` lowercases the tables in `icons/lookup_table.rs` and compiles them into three
[fst](https://docs.rs/fst) maps that are embedded in the binary.

## Check

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

CI runs the same three commands and builds the Docker image
(`.github/workflows/ci.yml`).

## Layout

| Path | What it holds |
| --- | --- |
| `src/lib.rs` | `get_icon_for_file`, `get_icon_for_folder` and their tests |
| `src/bin/iconator-server.rs` | the axum server and its HTTP tests |
| `build.rs` | turns `icons/lookup_table.rs` into the embedded fst maps |
| `icons/` | the SVG icons and the lookup table |

## Original assignment

> Please take no more than ~1 hour to complete this assignment. We’ll discuss your solutions in a call in a few days. Feel free to ask questions if the assignment is unclear at any point.

### Context

This repository contains a simple file-icon lookup library as may be used by IDEs for their project tree sidebar. `get_icon_for_file` and `get_icon_for_folder` accept a filepath and return the numeric ID of a corresponding icon if one is found. Each icon ID corresponds to an svg icon in `./icons`.

### Assignment

1. Implement a web server than allows users to query for file/folder icons over the network.
2. Write a short (<= 1 page) design/implementation note describing:
  - Design choices and tradeoffs you made (high-level architecture, web server choice, libraries you picked etc.)
  - How you would productionize this service (deployment, observability, etc.)

Keep your audience in mind. We’re not interested in a perfect solution but one where we see your thought-process and working style in action.

You may add these notes at the end of this file if you wish.
