# Mainmatter Rust Assessment

> Please take no more than ~1 hour to complete this assignment. We’ll discuss your solutions in a call in a few days. Feel free to ask questions if the assignment is unclear at any point.

## Context

This repository contains a simple file-icon lookup library as may be used by IDEs for their project tree sidebar. `get_icon_for_file` and `get_icon_for_folder` accept a filepath and return the numeric ID of a corresponding icon if one is found. Each icon ID corresponds to an svg icon in `./icons`.

## Assignment

1. Implement a web server than allows users to query for file/folder icons over the network. 
2. Write a short (<= 1 page) design/implementation note describing:
  - Design choices and tradeoffs you made (high-level architecture, web server choice, libraries you picked etc.)
  - How you would productionize this service (deployment, observability, etc.)
  
Keep your audience in mind. We’re not interested in a perfect solution but one where we see your thought-process and working style in action.
  
You may add these notes at the end of this file if you wish.

## Solution

An HTTP server lives in `src/bin/iconator-server.rs`. Design and production notes are in [DESIGN.md](DESIGN.md).

### Run

```sh
cargo run --release --bin iconator-server   # listens on 0.0.0.0:8080
HOST=127.0.0.1 PORT=3000 RUST_LOG=debug cargo run --bin iconator-server
```

### Query

```sh
curl 'http://localhost:8080/icons/file?path=src/main.rs'    # 200 {"icon_id":525}
curl 'http://localhost:8080/icons/folder?path=.github'      # 200 {"icon_id":862}
curl 'http://localhost:8080/icons/file?path=foo'            # 404 {"error":"not_found"}
curl 'http://localhost:8080/icons/file?path=..'             # 400 {"error":"invalid_path","message":"..."}
curl 'http://localhost:8080/health'                         # 200 {"status":"ok"}
```

### Check

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo audit
```
