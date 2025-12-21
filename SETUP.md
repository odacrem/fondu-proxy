# Setup Guide for macOS

Follow these steps to set up the development environment for `fondu-proxy` on a macOS machine.

## Prerequisites

- **Homebrew**: Ensure Homebrew is installed. If not, install it from [brew.sh](https://brew.sh/).

## 1. Install Rust Toolchain

Install Rust using `rustup`, the official installer:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

After installation, source the environment variables (or restart your terminal):

```bash
source $HOME/.cargo/env
```

## 2. Add WebAssembly Target

Fastly Compute requires the `wasm32-wasip1` target:

```bash
rustup target add wasm32-wasip1
```

## 3. Install Fastly CLI

Install the Fastly CLI via Homebrew:

```bash
brew install fastly/tap/fastly
```

## 4. Install Testing Tools

To run the unit tests in a WebAssembly environment (WASI), you need `wasmtime`:

```bash
curl https://wasmtime.dev/install.sh -sSf | bash
```

(Optional but recommended) Install `cargo-wasi`:

```bash
cargo install cargo-wasi
```

## 5. Project Configuration

Create a `.env` file in the project directory (required by the `Makefile`):

```bash
touch .env
```

## 6. Verify Installation

Run the following commands to ensure everything is set up correctly:

### Build the Project
```bash
make build
# or
fastly compute build
```

### Run Unit Tests
```bash
CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime" cargo test --target wasm32-wasip1
# or (if Makefile is updated)
make test
```

### Local Development Server
```bash
make serve
# or
fastly compute serve
```
