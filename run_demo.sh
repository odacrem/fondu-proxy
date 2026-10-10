#!/bin/bash

# Kill background jobs on exit, allowing them to shut down gracefully
cleanup() {
  # Get all background job PIDs, if any
  local pids
  pids=$(jobs -p) || return 0
  [ -z "$pids" ] && return 0

  # First, try to terminate gracefully
  kill -TERM $pids 2>/dev/null || true

  # Wait up to 5 seconds for processes to exit
  local end=$((SECONDS + 5))
  while kill -0 $pids 2>/dev/null; do
    if [ "$SECONDS" -ge "$end" ]; then
      # Force kill any remaining processes
      kill -KILL $pids 2>/dev/null || true
      break
    fi
    sleep 0.5
  done

  # Reap any finished background jobs
  wait 2>/dev/null || true
}

trap cleanup EXIT

# Silence rustup updates to prevent breaking Fastly CLI metadata parsing
export RUSTUP_QUIET=1

echo "Starting Content Backend on port 3000..."
python3 -m http.server 3000 --directory demo/content &

echo "Starting Fondu Backend on port 4000..."
python3 demo/fondu/server.py 4000 &

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
