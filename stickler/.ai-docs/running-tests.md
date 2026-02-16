# Running Stickler Tests - Quick Reference for Claude

This document provides quick commands and automation scripts for running Stickler tests efficiently.

## Quick Test Commands

### Run Live Query Test (Full Flow)
```bash
cd /home/nickthorpe71/projects/scrapingbee_mvp/stickler
npm run cli run live-query
```

### Clean Old Test Runs
```bash
cd /home/nickthorpe71/projects/scrapingbee_mvp/stickler
npm run cli clean --all
```

### Clean and Run Fresh Test
```bash
cd /home/nickthorpe71/projects/scrapingbee_mvp/stickler
npm run cli clean --all && npm run cli run live-query
```

## Automated Test Script

When running tests with automation (to avoid manual planning responses), use this script:

```bash
#!/bin/bash
# Save as: /tmp/stickler_auto_test.sh

# Get the most recent run directory
RUN_DIR=$(ls -td /home/nickthorpe71/projects/scrapingbee_mvp/stickler/runs/*live-query 2>/dev/null | head -1)
echo "Using run dir: $RUN_DIR"

# Login flow
sleep 2 && echo '{"type":"click","target":"Login","reason":"Navigate to login"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"wait","duration":1000,"reason":"Wait for page load"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"type","target":"Enter your email address","text":"mpenczak25@gmail.com","reason":"Enter email"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"click","target":"Continue","reason":"Submit email"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"type","target":"Enter your password","text":"TestTest321321","reason":"Enter password"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"click","target":"Continue","reason":"Submit password"}' > "$RUN_DIR/planning_response.json"

# Wait for auth
sleep 3 && echo '{"type":"wait","duration":1000,"reason":"Wait for auth"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"wait","duration":1000,"reason":"Wait for page load"}' > "$RUN_DIR/planning_response.json"

# Fill query form
sleep 3 && echo '{"type":"type","target":"e.g., DeWalt, Milwaukee","text":"Dell","reason":"Enter manufacturer"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"type","target":"e.g., DEWALT DCD200D1 20V MAX Brushless Drain Snake KitB, M12 12-V Lithium-Ion Cordless Drain Snake Auger W/ (1) 1.5Ah Battery, 5/16 in. x 25 ft. Cable, Charger, & 5 Gal. Bucket","text":"Dell XPS 15 9500 laptop","reason":"Enter product"}' > "$RUN_DIR/planning_response.json"

# Execute query
sleep 3 && echo '{"type":"click","target":"COMPARE","reason":"Execute query"}' > "$RUN_DIR/planning_response.json"

# Wait for AI to complete (60 seconds total - AI typically takes 40-45 seconds)
sleep 3 && echo '{"type":"wait","duration":15000,"reason":"AI processing"}' > "$RUN_DIR/planning_response.json"
sleep 17 && echo '{"type":"wait","duration":15000,"reason":"AI processing"}' > "$RUN_DIR/planning_response.json"
sleep 17 && echo '{"type":"wait","duration":15000,"reason":"AI processing"}' > "$RUN_DIR/planning_response.json"
sleep 17 && echo '{"type":"wait","duration":15000,"reason":"AI processing"}' > "$RUN_DIR/planning_response.json"

# Scroll to view results
sleep 17 && echo '{"type":"scroll","direction":"down","amount":300,"reason":"View source product details"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"scroll","direction":"down","amount":400,"reason":"View matched products"}' > "$RUN_DIR/planning_response.json"

# Let verifier check, then mark complete
sleep 3 && echo '{"type":"wait","duration":2000,"reason":"Let verifier check"}' > "$RUN_DIR/planning_response.json"
sleep 3 && echo '{"type":"done","reason":"Test complete - AI results verified"}' > "$RUN_DIR/planning_response.json"
```

## Running a Complete Automated Test

1. Start the test in background:
```bash
cd /home/nickthorpe71/projects/scrapingbee_mvp/stickler
npm run cli run live-query &
```

2. In a separate command, run the automation script:
```bash
# Create and run the script
cat > /tmp/stickler_auto.sh << 'EOF'
[paste script from above]
EOF
chmod +x /tmp/stickler_auto.sh
/tmp/stickler_auto.sh &
```

3. Monitor progress:
```bash
# Watch the test output
tail -f /home/nickthorpe71/projects/scrapingbee_mvp/stickler/runs/*/trace.jsonl

# Or check specific screenshots
ls -lt /home/nickthorpe71/projects/scrapingbee_mvp/stickler/runs/*/screenshots/
```

## Expected Test Results

### Successful Test Metrics
- **Duration**: ~295 seconds (4m 55s)
- **Steps**: 19-20 steps
- **AI Processing Time**: ~41 seconds
- **Final Result**: "Completed in 41s - 3 matches"

### Key Screenshots to Check
- **Step 015**: AI results should start appearing (visible text count increases from 18 to 20)
- **Step 018**: Full results visible with "3 matches" and product details
- Look for: "Completed in [X]s 3 matches" in sidebar
- Look for: "MATCHED PRODUCTS" section with "LINKS USED TO FIND MATCHES"

## Troubleshooting

### Test fails at password screen (Step 5)
- Issue: Page not loaded yet when observer captures state
- Solution: Timeouts are set to 3000ms which should work. If failing, wait times may need adjustment.

### AI processing takes too long
- Current wait time: 60 seconds (four 15-second waits)
- AI typically completes in 40-45 seconds
- If consistently timing out, increase wait duration in automation script

### Success signals not detected
- Current signals: "Results", "found", "match"
- Note: UI shows "FIND" not "found" - may need to update scenario signals
- Workaround: Use "done" action when results are visually confirmed

## Performance Optimizations Applied

Current speed settings (optimized from original):
- Observer timeout: 5000ms → **3000ms**
- Network idle timeout: 3000ms → **1000ms**
- Action delay: 100-500ms → **50-100ms**
- Planner poll interval: 2000ms → **500ms**

**Result**: ~32% faster per step, ~14% faster overall test time
