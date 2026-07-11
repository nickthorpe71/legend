#!/usr/bin/env bash
# Phase 0 end-to-end. Runs preflight (which flips models_verified once the
# configured GPT-5.6 IDs actually resolve + round-trip), wipes prior artifacts,
# then fetch -> ingest -> arms -> grade -> report. Any stage failing aborts.
#
# First run with config n_questions=1..5 (smoke). Flip to 50 for the real run.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck disable=SC1091
source .venv/bin/activate

echo "== preflight (verifies key + model IDs) =="
python preflight.py --write

echo "== wipe generated artifacts =="
rm -rf corpus store runs results

echo "== step 2: fetch_corpus =="
python fetch_corpus.py

echo "== step 3: ingest (Sol) =="
python ingest.py

echo "== step 4: run_arms (Terra x2) =="
python run_arms.py

echo "== step 5: grade (Sol) =="
python grade.py

echo "== step 6: report =="
python report.py

echo
echo "Done. Read results/report.md, then verify every flip in results/flips.md."
