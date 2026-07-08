#!/usr/bin/env python3
"""Faithful MCP test: a real model drives a live `legend mcp-serve` over the
actual protocol. The model is given the server's own tools/list schema and
`instructions` (nothing hand-written here), and every tool call is routed to
the server as a real tools/call. This is the honest "model uses the MCP" test.

  python3 harness/mcp_client_test.py [--model claude-sonnet-5]
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, json, subprocess, tempfile, urllib.request, urllib.error
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PRICES = {"claude-haiku-4-5-20251001": (1.0, 5.0), "claude-sonnet-5": (3.0, 15.0),
          "claude-opus-4-8": (15.0, 75.0)}


class MCP:
    """Minimal MCP stdio client for one server subprocess."""
    def __init__(self, cmd, env):
        self.p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  env=env, text=True, bufsize=1)
        self._id = 0

    def _send(self, obj):
        self.p.stdin.write(json.dumps(obj) + "\n"); self.p.stdin.flush()

    def request(self, method, params=None):
        self._id += 1
        self._send({"jsonrpc": "2.0", "id": self._id, "method": method,
                    **({"params": params} if params is not None else {})})
        line = self.p.stdout.readline()
        return json.loads(line)["result"]

    def notify(self, method):
        self._send({"jsonrpc": "2.0", "method": method})

    def call_tool(self, name, arguments):
        r = self.request("tools/call", {"name": name, "arguments": arguments})
        txt = "".join(b.get("text", "") for b in r.get("content", []))
        return txt, r.get("isError", False)

    def close(self):
        try:
            self.p.stdin.close(); self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


def api(model, system, messages, tools):
    body = {"model": model, "max_tokens": 1024, "system": system,
            "messages": messages, "tools": tools}
    req = urllib.request.Request("https://api.anthropic.com/v1/messages",
        data=json.dumps(body).encode(),
        headers={"x-api-key": os.environ["ANTHROPIC_API_KEY"],
                 "anthropic-version": "2023-06-01", "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as r:
        d = json.load(r)
    u = d.get("usage", {})
    return d, u.get("input_tokens", 0), u.get("output_tokens", 0)


# A realistic multi-turn conversation that hits save -> supersede -> recall,
# the exact arc the old hand-rolled harness could not express.
SCENARIO = [
    "I'm starting a project called Alchamancer 2 — a solo-mage tactical-grid roguelike, "
    "written from scratch in C. Target run length is about 60-90 minutes. Remember this.",
    "Update: after playtests we cut the run length. It's now 16-18 waves, roughly 40-50 minutes. "
    "Also, the mana economy starts at 0 of every color.",
    "Quick check — what's the current run length of Alchamancer 2, and what was it before?",
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="claude-sonnet-5")
    args = ap.parse_args()
    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("ANTHROPIC_API_KEY not set")
    ir, orr = PRICES.get(args.model, (0.0, 0.0))

    tmp = Path(tempfile.mkdtemp(prefix="legend-mcptest-"))
    legend = tmp / "legend"
    if subprocess.run(["gcc", "-std=c99", "-O2", "legend.c", "embed.c", "-o", str(legend), "-lm"],
                      cwd=ROOT, capture_output=True, text=True).returncode:
        sys.exit("build failed")
    store = tmp / "store"
    env = dict(os.environ, LEGEND_STATE_DIR=str(store), LEGEND_EMBED_DIR=str(ROOT / "models/bge-small-en-v1.5"))
    subprocess.run([str(legend), "init"], env=env, capture_output=True, cwd=ROOT)

    mcp = MCP([str(legend), "mcp-serve"], env)
    init = mcp.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}})
    system = init.get("instructions", "")
    print(f"→ connected: {init['serverInfo']}  ({len(system)}-char instructions)")
    mcp.notify("notifications/initialized")
    mtools = mcp.request("tools/list")["tools"]
    tools = [{"name": t["name"], "description": t["description"], "input_schema": t["inputSchema"]}
             for t in mtools]
    print(f"→ tools advertised: {[t['name'] for t in tools]}\n")

    messages, tin, tout = [], 0, 0
    for turn, user_text in enumerate(SCENARIO, 1):
        print(f"{'='*70}\n🧑 USER (turn {turn}): {user_text}\n")
        messages.append({"role": "user", "content": user_text})
        while True:
            d, ci, co = api(args.model, system, messages, tools)
            tin += ci; tout += co
            blocks = d.get("content", [])
            messages.append({"role": "assistant", "content": blocks})
            for b in blocks:
                if b.get("type") == "text" and b["text"].strip():
                    print(f"🤖 {b['text'].strip()}")
            calls = [b for b in blocks if b.get("type") == "tool_use"]
            if not calls:
                break
            results = []
            for c in calls:
                out, is_err = mcp.call_tool(c["name"], c["input"])
                short = out.replace("\n", " ")[:160]
                print(f"   🔧 {c['name']} INPUT: {json.dumps(c['input'])}")
                print(f"      -> {'ERR ' if is_err else ''}{short}")
                results.append({"type": "tool_result", "tool_use_id": c["id"],
                                "content": out, "is_error": is_err})
            messages.append({"role": "user", "content": results})
        print()

    # Objective evidence: query the substrate DIRECTLY (no model, no chat
    # context) with history so we see what Legend itself retained — current
    # value plus the superseded prior.
    print("=" * 70)
    print("SUBSTRATE (direct recall, history_depth=3 — no model in the loop):")
    frame, _ = mcp.call_tool("legend_recall", {"focus": ["Alchamancer 2"], "history_depth": 3})
    print(json.dumps(json.loads(frame), indent=2))

    total = tin / 1e6 * ir + tout / 1e6 * orr
    print("=" * 70)
    print(f"model {args.model}  COST ${total:.4f}  ({tin} in + {tout} out tok)")
    mcp.close()
    import shutil; shutil.rmtree(tmp)


if __name__ == "__main__":
    main()
