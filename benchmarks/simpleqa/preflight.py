#!/usr/bin/env python3
"""Step 0 preflight: verify everything the run depends on before spending tokens.

Local checks always run. OpenAI checks run only if a key is present (they are
what actually validate the key + resolve the real GPT-5.6 model IDs). Nothing
here mutates state except an optional --write that flips models_verified once
the configured IDs resolve and a tool round-trip succeeds.

    python preflight.py            # report
    python preflight.py --write    # also set models_verified:true if all green
"""

import argparse
import json
import sys
import tempfile
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import oai, prompts  # noqa: E402
from common.legend_io import Legend  # noqa: E402
from common.util import load_config  # noqa: E402

UPSTREAM_GRADER = (
    "https://raw.githubusercontent.com/openai/simple-evals/main/simpleqa_eval.py"
)


class Check:
    def __init__(self):
        self.rows = []
        self.failed = False

    def add(self, ok, name, detail=""):
        self.rows.append((ok, name, detail))
        if ok is False:
            self.failed = True

    def report(self):
        print("\n=== preflight ===")
        for ok, name, detail in self.rows:
            mark = {True: "PASS", False: "FAIL", None: "SKIP"}[ok]
            print(f"  [{mark}] {name}" + (f" — {detail}" if detail else ""))
        print("=================")
        return not self.failed


def local_checks(cfg, chk):
    # legend binary + init round-trip
    binary = Path(cfg["legend_binary"])
    chk.add(binary.exists(), "legend binary present", str(binary))
    if binary.exists():
        try:
            with tempfile.TemporaryDirectory() as tmp:
                lg = Legend(binary, Path(tmp) / ".legend", now=cfg.get("legend_now"), embed=cfg.get("legend_embed", 1))
                out = lg.init(reset=True)
                chk.add(out.get("elements", 0) >= 32, "legend init round-trip", f"{out.get('elements')} elements seeded")
        except Exception as e:
            chk.add(False, "legend init round-trip", f"{type(e).__name__}: {e}")

    # bge model dir
    embed_dir = Path(cfg["legend_binary"]).parent / "models" / "bge-small-en-v1.5"
    chk.add(embed_dir.is_dir(), "bge embedding model dir", str(embed_dir))

    # dataset reachable (HEAD)
    try:
        req = urllib.request.Request(cfg["dataset_url"], method="HEAD",
                                     headers={"User-Agent": cfg["http_user_agent"]})
        with urllib.request.urlopen(req, timeout=20) as r:
            chk.add(r.status == 200, "SimpleQA dataset reachable", f"HTTP {r.status}")
    except Exception as e:
        chk.add(False, "SimpleQA dataset reachable", f"{type(e).__name__}: {e}")

    # grader template diff vs upstream
    try:
        req = urllib.request.Request(UPSTREAM_GRADER, headers={"User-Agent": cfg["http_user_agent"]})
        with urllib.request.urlopen(req, timeout=20) as r:
            upstream = r.read().decode("utf-8")
        # A couple of distinctive anchor lines must appear verbatim in upstream.
        anchors = [
            "assign a grade of either",
            'Just return the letters "A", "B", or "C"',
        ]
        ok = all(a in upstream for a in anchors)
        chk.add(ok, "grader template anchors match upstream",
                "ok" if ok else "upstream text drifted — re-copy GRADER_TEMPLATE")
    except Exception as e:
        chk.add(None, "grader template anchors match upstream", f"skipped: {type(e).__name__}")


def openai_checks(cfg, chk, write):
    import os
    oai.load_dotenv()
    if not os.environ.get("OPENAI_API_KEY"):
        chk.add(None, "OPENAI_API_KEY present", "add to repo .env to run OpenAI checks")
        return
    chk.add(True, "OPENAI_API_KEY present")

    # list models and surface gpt-5* candidates
    try:
        candidates = oai.list_models(prefix="gpt-5")
        print("\n  gpt-5* models visible to this key:")
        for c in candidates:
            print(f"    - {c}")
        all_models = set(oai.list_models())
    except Exception as e:
        chk.add(False, "list models", f"{type(e).__name__}: {e}")
        return

    ids = cfg["models"]
    configured = {k: ids[k] for k in ("ingester", "consumer", "grader")}
    unresolved = {role: mid for role, mid in configured.items() if mid not in all_models}
    if unresolved:
        chk.add(False, "configured model IDs resolve",
                "NOT FOUND: " + ", ".join(f"{r}={m}" for r, m in unresolved.items())
                + " — pick real IDs from the gpt-5* list above and edit config.json")
        return
    chk.add(True, "configured model IDs resolve", ", ".join(sorted(set(configured.values()))))

    # tool round-trip on the ingester model (validates tool-call API shape)
    probe_tool = {
        "type": "function",
        "function": {
            "name": "echo",
            "description": "Echo the token back.",
            "parameters": {
                "type": "object",
                "properties": {"token": {"type": "string"}},
                "required": ["token"],
                "additionalProperties": False,
            },
        },
    }
    try:
        seen = {}
        def dispatch(name, args):
            seen["token"] = args.get("token")
            return json.dumps({"ok": True})
        res = oai.run_tool_loop(
            model=configured["ingester"],
            system="You are a test harness probe.",
            user="Call the echo tool with token='ping', then reply DONE.",
            tools=[probe_tool],
            dispatch=dispatch,
            max_calls=3,
            seed=cfg.get("seed"),
        )
        ok = seen.get("token") == "ping"
        chk.add(ok, "ingester tool round-trip",
                f"answer={res['answer'][:40]!r} calls={res['tool_calls_made']}")
    except Exception as e:
        chk.add(False, "ingester tool round-trip", f"{type(e).__name__}: {e}")
        return

    # plain completion on the grader model
    try:
        text, _ = oai.complete(configured["grader"], "", "Reply with exactly: OK", seed=cfg.get("seed"))
        chk.add("OK" in text.upper(), "grader completion", repr(text[:40]))
    except Exception as e:
        chk.add(False, "grader completion", f"{type(e).__name__}: {e}")

    if write and not chk.failed:
        cfg_path = Path(__file__).resolve().parent / "config.json"
        raw = json.loads(cfg_path.read_text())
        raw["models"]["models_verified"] = True
        cfg_path.write_text(json.dumps(raw, indent=2) + "\n")
        print("\n  models_verified set to true in config.json")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--write", action="store_true", help="flip models_verified if all green")
    args = ap.parse_args()

    cfg = load_config()
    chk = Check()
    local_checks(cfg, chk)
    openai_checks(cfg, chk, args.write)
    ok = chk.report()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
