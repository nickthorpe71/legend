"""Subprocess wrapper around the Legend C binary.

Every call shells out to `legend <verb>` with the store bound through
LEGEND_STATE_DIR and a fixed LEGEND_NOW clock (determinism seam for replay).
Payloads go in on stdin as JSON; output is parsed as JSON. A non-zero exit
means the binary printed a structured `{"error": {"code", "message"}}` object,
which we surface as a LegendError so callers branch on the code, not prose.
"""

import json
import os
import subprocess
from pathlib import Path


class LegendError(RuntimeError):
    def __init__(self, code, message, verb):
        self.code = code
        self.message = message
        self.verb = verb
        super().__init__(f"legend {verb} failed [{code}]: {message}")


class Legend:
    def __init__(self, binary, store_dir, now=None, embed=1, embed_dir=None):
        self.binary = str(binary)
        self.store_dir = str(Path(store_dir).resolve())
        self.now = now
        self.embed = embed
        self.embed_dir = embed_dir

    def _env(self):
        env = dict(os.environ)
        env["LEGEND_STATE_DIR"] = self.store_dir
        if self.now is not None:
            env["LEGEND_NOW"] = str(self.now)
        env["LEGEND_EMBED"] = str(self.embed)
        if self.embed_dir:
            env["LEGEND_EMBED_DIR"] = str(self.embed_dir)
        return env

    def _run(self, verb, payload=None, extra_args=()):
        args = [self.binary, verb, *extra_args]
        stdin = json.dumps(payload) if payload is not None else ""
        proc = subprocess.run(
            args,
            input=stdin,
            capture_output=True,
            text=True,
            env=self._env(),
        )
        out = proc.stdout.strip()
        if proc.returncode != 0:
            code, message = "unknown", (proc.stderr.strip() or out)
            try:
                err = json.loads(out).get("error", {})
                code = err.get("code", code)
                message = err.get("message", message)
            except (json.JSONDecodeError, AttributeError):
                pass
            raise LegendError(code, message, verb)
        return json.loads(out) if out else {}

    def init(self, reset=False):
        # the binary creates the .legend dir but not its parent
        Path(self.store_dir).parent.mkdir(parents=True, exist_ok=True)
        return self._run("init", None, ("--reset",) if reset else ())

    def save(self, payload):
        return self._run("save", payload)

    def recall(self, payload):
        return self._run("recall", payload)

    def dump(self, pretty=False):
        return self._run("dump", None, ("--pretty",) if pretty else ())


def from_config(cfg, store_dir):
    """Build a Legend bound to `store_dir` from a loaded config dict.

    Pin LEGEND_EMBED_DIR to the model dir next to the binary. Without this the
    binary resolves the default "models/..." relative to the process CWD (the
    benchmark dir, where it does not exist) and recall silently runs with
    embeddings OFF — no semantic resolution, no relevance ranking.
    """
    embed_dir = Path(cfg["legend_binary"]).parent / "models" / "bge-small-en-v1.5"
    return Legend(
        binary=cfg["legend_binary"],
        store_dir=store_dir,
        now=cfg.get("legend_now"),
        embed=cfg.get("legend_embed", 1),
        embed_dir=str(embed_dir) if embed_dir.is_dir() else None,
    )


if __name__ == "__main__":
    # Smoke test: temp store round-trip. Run:
    #   python3 -m common.legend_io /path/to/legend
    import sys
    import tempfile

    binary = sys.argv[1] if len(sys.argv) > 1 else "/home/nickthorpe71/Code/legend/legend"
    with tempfile.TemporaryDirectory() as tmp:
        lg = Legend(binary, Path(tmp) / ".legend", now=1720000000, embed=1)
        print("init:", lg.init(reset=True).get("elements"), "elements")
        lg.save(
            {
                "source": "smoke",
                "elements": [{"name": "Marie Curie", "kind": "person", "summary": "physicist"}],
                "facts": [{"s": "Marie Curie", "p": "nobel_year", "o": "1903", "src": "wiki:1"}],
            }
        )
        frame = lg.recall({"focus": ["Marie Curie"], "limit": 5})
        print("recall focus:", [f["name"] for f in frame.get("focus", [])])
        print("recall recent:", frame.get("recent"))
        dump = lg.dump()
        print("dump elements:", len(dump.get("elements", [])))
        missing = Legend(binary, Path(tmp) / "no-such-store", now=1720000000)
        try:
            missing.recall({"focus": ["x"]})
        except LegendError as e:
            print("error path ok:", e.code)
        print("SMOKE OK")
