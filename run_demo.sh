#!/bin/bash

# Kill background jobs on exit
trap 'kill $(jobs -p)' EXIT

# Silence rustup updates to prevent breaking Fastly CLI metadata parsing
export RUSTUP_QUIET=1

echo "Starting Content Backend on port 3000..."
python3 -m http.server 3000 --directory demo/content &

echo "Starting Fondu Backend on port 4000..."
python3 -m http.server 4000 --directory demo/fondu &

# Give them a moment to spin up
sleep 1

echo "---------------------------------------------------"
echo "✅  Backends running!"
echo "🚀 Starting Fastly Proxy..."
echo "    - Proxy:   http://127.0.0.1:7676"
echo "    - Content: http://localhost:3000"
echo "    - Fondu:   http://localhost:4000"
echo "---------------------------------------------------"

# Run the proxy in the foreground
fastly compute serve
