#!/bin/bash
cd /home/nickthorpe71/code/instill/stickler

SCENARIO="${1:-pdf-upload-ui}"

# Kill any existing tests
pkill -f "tsx src/cli"
sleep 2

# Start test in background
npm run cli run "$SCENARIO" > /tmp/test.log 2>&1 &
TEST_PID=$!

echo "Test started with PID $TEST_PID"
sleep 5

# Find the latest run directory
RUN_DIR=$(ls -td runs/*${SCENARIO}* 2>/dev/null | head -1)
echo "Run directory: $RUN_DIR"

# Wait for planning request
for i in {1..10}; do
  if [ -f "$RUN_DIR/planning_request.json" ]; then
    echo "Planning request found at step $i"

    # Check URL
    if grep -q '"url": "http://localhost:5173/"' "$RUN_DIR/planning_request.json"; then
      echo '{"type":"click","target":"Upload PDF","reason":"Navigate to upload page"}' > "$RUN_DIR/planning_response.json"
      echo "Response: Click Upload PDF"
      sleep 5
    fi

    if grep -q '/upload' "$RUN_DIR/planning_request.json"; then
      echo '{"type":"done","reason":"Upload page loaded"}' > "$RUN_DIR/planning_response.json"
      echo "Response: Done"
      sleep 3
    fi
  fi

  if [ -f "$RUN_DIR/verdict.json" ]; then
    echo "=== VERDICT ==="
    cat "$RUN_DIR/verdict.json"
    exit 0
  fi

  sleep 2
done

echo "Timeout waiting for test completion"
