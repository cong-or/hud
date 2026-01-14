#!/bin/bash
# Continuous load generator for demo-server
# Usage: ./load.sh [requests_per_second]

RPS=${1:-10}
DELAY=$(echo "scale=3; 1/$RPS" | bc)

echo "Generating load at ~$RPS req/s (Ctrl+C to stop)"
echo "Endpoints: /hash, /compress, /read, /dns"
echo ""

while true; do
  # Rotate through endpoints
  curl -s -X POST http://localhost:3000/hash -d 'password123' > /dev/null &
  curl -s -X POST http://localhost:3000/compress -d 'compress this data please' > /dev/null &
  curl -s http://localhost:3000/read > /dev/null &
  curl -s http://localhost:3000/dns > /dev/null &

  sleep "$DELAY"
done
