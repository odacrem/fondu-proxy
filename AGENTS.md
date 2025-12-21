# Agent Guide: Fondu Proxy

This document is intended for AI coding agents to quickly understand the project structure, architecture, and development workflow.

## Project Overview

**Fondu Proxy** is a High-Performance HTML rewriter built on **Fastly Compute (Rust)**. It performs "content stitching" at the edge, similar to parallel Edge Side Includes (ESI).

### Core Logic
1.  **Incoming Request**: Captured by `src/main.rs`.
2.  **Async Subrequests**: Proxies the request to two backends simultaneously:
    -   `content`: The source HTML provider.
    -   `fondu`: A JSON API providing content directives (selectors and HTML snippets).
3.  **Streaming Rewrite**: Uses the `lol_html` crate to parse the origin HTML and apply operations (Replace, Append, Prepend, Before, After) defined by the `fondu` backend.
4.  **Edge Delivery**: Returns the reconstructed HTML to the client.

## Repository Structure

- `src/main.rs`: Entry point, request routing, and backend orchestration.
- `src/fondu.rs`: HTML rewriting logic using `lol_html`.
- `fastly.toml`: Deployment and local development configuration.
- `Cargo.toml`: Project dependencies (`fastly`, `lol_html`, `serde`, etc.).
- `Makefile`: Human-friendly wrappers for build/test commands.
- `SETUP.md`: Detailed environment setup guide for macOS.

## Environment Requirements

- **Runtime**: Fastly Compute (WebAssembly).
- **Target**: `wasm32-wasip1`.
- **CLI**: `fastly` (Fastly CLI).
- **Test Runner**: `wasmtime` (required for WASI-compliant unit testing).

## Key Commands

- **Build**: `fastly compute build` (compiles to `.wasm` package).
- **Test**: `CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime" cargo test --target wasm32-wasip1` (runs unit tests in `src/main.rs`).
- **Serve**: `fastly compute serve` (spins up a local server on `localhost:4000`).

## Configuration Notes

- **Backends**: Requires `content` and `fondu` backends to be defined in `fastly.toml` or environment.
- **Config Store**: Uses a dictionary named `config` with a `fondu_path` key to determine where to fetch component data.
- **WASI**: The project targets `wasip1`. Avoid standard library features not supported in the WASI environment (e.g., synchronous file I/O).

## For Implementation
When modifying the HTML rewriting logic, focus on `src/fondu.rs`. All request/response handling and backend proxying are managed in `src/main.rs`.
