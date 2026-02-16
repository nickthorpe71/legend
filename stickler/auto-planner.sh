#!/bin/bash
# Auto-planner script that watches for planning requests and responds

RUN_DIR="$1"

if [ -z "$RUN_DIR" ]; then
  echo "Usage: $0 <run-directory>"
  exit 1
fi

echo "Watching $RUN_DIR for planning requests..."

while true; do
  if [ -f "$RUN_DIR/planning_request.json" ] && [ ! -f "$RUN_DIR/planning_response.json" ]; then
    echo "Planning request detected!"

    # Read the current state
    URL=$(grep -o '"url": "[^"]*"' "$RUN_DIR/planning_request.json" | head -1 | cut -d'"' -f4)
    STEP=$(grep -o '"step": [0-9]*' "$RUN_DIR/planning_request.json" | head -1 | awk '{print $2}')

    echo "Step $STEP - URL: $URL"

    # Simple logic: if on landing page, click Upload PDF
    if [[ "$URL" == *"localhost:5173/"* ]] && [[ "$URL" != *"/upload"* ]]; then
      echo '{"type":"click","target":"Upload PDF","reason":"Navigate to upload page"}' > "$RUN_DIR/planning_response.json"
      echo "Responded: Click Upload PDF"
    elif [[ "$URL" == *"/upload"* ]]; then
      # On upload page - mark as done
      echo '{"type":"done","reason":"Upload page loaded with Choose File button visible"}' > "$RUN_DIR/planning_response.json"
      echo "Responded: Done"
    else
      # Fallback
      echo '{"type":"wait","duration":1000,"reason":"Observing page state"}' > "$RUN_DIR/planning_response.json"
      echo "Responded: Wait"
    fi

    sleep 2
  fi

  # Check if verdict exists (test complete)
  if [ -f "$RUN_DIR/verdict.json" ]; then
    echo "Test complete! Verdict:"
    cat "$RUN_DIR/verdict.json"
    exit 0
  fi

  sleep 1
done
