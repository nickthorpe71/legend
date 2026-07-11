"""OpenAI client: one place for every GPT-5.6 call.

Everything the harness sends to OpenAI goes through here so the API shape lives
in a single file. If GPT-5.6 turns out to need the Responses API or a different
parameter name, this is the only module to change. We deliberately do NOT send
`temperature` or `max_tokens`: newer reasoning models reject non-default
temperature and renamed the token cap, and Phase 0 does not need either.
"""

import json
import os
import time
from pathlib import Path

try:
    import openai
    from openai import OpenAI
except ImportError as e:  # pragma: no cover
    raise SystemExit(
        "openai SDK not installed. Run: pip install -r benchmarks/simpleqa/requirements.txt"
    ) from e


REPO_ROOT = Path(__file__).resolve().parents[3]


def load_dotenv(path=None):
    """Merge KEY=VALUE lines from the repo .env into os.environ (no overwrite)."""
    path = Path(path) if path else REPO_ROOT / ".env"
    if not path.exists():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, val = line.partition("=")
        key, val = key.strip(), val.strip().strip('"').strip("'")
        os.environ.setdefault(key, val)


_CLIENT = None


def client():
    global _CLIENT
    if _CLIENT is None:
        load_dotenv()
        key = os.environ.get("OPENAI_API_KEY")
        if not key:
            raise SystemExit(
                "OPENAI_API_KEY not set. Add it to the repo .env (gitignored) or export it."
            )
        kwargs = {"api_key": key}
        base = os.environ.get("OPENAI_BASE_URL")
        if base:
            kwargs["base_url"] = base
        _CLIENT = OpenAI(**kwargs)
    return _CLIENT


def _retryable():
    names = ("RateLimitError", "APITimeoutError", "APIConnectionError", "InternalServerError")
    return tuple(getattr(openai, n) for n in names if hasattr(openai, n))


def _create(model, messages, tools=None, tool_choice=None, seed=None, max_retries=6):
    """One chat.completions call with exponential backoff on transient errors."""
    kwargs = {"model": model, "messages": messages}
    if tools:
        kwargs["tools"] = tools
        kwargs["tool_choice"] = tool_choice or "auto"
    if seed is not None:
        kwargs["seed"] = seed
    retryable = _retryable()
    delay = 2.0
    for attempt in range(max_retries):
        try:
            return client().chat.completions.create(**kwargs)
        except retryable as e:
            if attempt == max_retries - 1:
                raise
            time.sleep(delay)
            delay = min(delay * 2, 60.0)
        except openai.APIStatusError as e:  # 5xx that isn't one of the above
            if e.status_code and 500 <= e.status_code < 600 and attempt < max_retries - 1:
                time.sleep(delay)
                delay = min(delay * 2, 60.0)
                continue
            raise


def _usage(resp):
    u = getattr(resp, "usage", None)
    if not u:
        return {}
    try:
        return u.model_dump()
    except AttributeError:
        return dict(u) if isinstance(u, dict) else {}


def complete(model, system, user, seed=None):
    """Single-shot completion, no tools. Returns (text, usage)."""
    messages = []
    if system:
        messages.append({"role": "system", "content": system})
    messages.append({"role": "user", "content": user})
    resp = _create(model, messages, seed=seed)
    text = resp.choices[0].message.content or ""
    return text.strip(), _usage(resp)


def _assistant_dict(msg):
    d = {"role": "assistant", "content": msg.content}
    if msg.tool_calls:
        d["tool_calls"] = [
            {
                "id": tc.id,
                "type": "function",
                "function": {"name": tc.function.name, "arguments": tc.function.arguments},
            }
            for tc in msg.tool_calls
        ]
    return d


def run_tool_loop(model, system, user, tools, dispatch, max_calls=8, seed=None):
    """Drive a tool-calling conversation until the model stops calling tools.

    `dispatch(name, args_dict) -> result_str` executes one tool call.
    Returns dict: {answer, messages, trace, usage_total, tool_calls_made,
    stop_reason}. The loop is bounded by max_calls; if the model is still
    calling tools at the cap we return the last assistant content (possibly
    empty) with stop_reason="max_calls".
    """
    messages = []
    if system:
        messages.append({"role": "system", "content": system})
    messages.append({"role": "user", "content": user})

    trace = []
    usage_total = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    calls_made = 0

    def _accumulate(u):
        for k in usage_total:
            usage_total[k] += int(u.get(k, 0) or 0)

    while True:
        resp = _create(model, messages, tools=tools, seed=seed)
        _accumulate(_usage(resp))
        msg = resp.choices[0].message
        messages.append(_assistant_dict(msg))

        if not msg.tool_calls:
            return {
                "answer": (msg.content or "").strip(),
                "messages": messages,
                "trace": trace,
                "usage_total": usage_total,
                "tool_calls_made": calls_made,
                "stop_reason": "stop",
            }

        for tc in msg.tool_calls:
            name = tc.function.name
            try:
                args = json.loads(tc.function.arguments or "{}")
            except json.JSONDecodeError:
                args = {}
            calls_made += 1
            over_cap = calls_made > max_calls
            if over_cap:
                result = json.dumps({"error": "tool-call budget exhausted; answer now with what you have"})
            else:
                try:
                    result = dispatch(name, args)
                except Exception as e:  # dispatch failure must not kill the loop
                    result = json.dumps({"error": f"{type(e).__name__}: {e}"})
            trace.append({"call": name, "args": args, "result": result, "over_cap": over_cap})
            messages.append({"role": "tool", "tool_call_id": tc.id, "content": result})

        if calls_made >= max_calls:
            # One more turn to let the model answer with what it has, no tools.
            resp = _create(model, messages, seed=seed)
            _accumulate(_usage(resp))
            final = resp.choices[0].message
            messages.append(_assistant_dict(final))
            return {
                "answer": (final.content or "").strip(),
                "messages": messages,
                "trace": trace,
                "usage_total": usage_total,
                "tool_calls_made": calls_made,
                "stop_reason": "max_calls",
            }


def list_models(prefix=None):
    """Return model IDs, optionally filtered by prefix. Used by preflight."""
    ids = [m.id for m in client().models.list().data]
    if prefix:
        ids = [i for i in ids if i.startswith(prefix)]
    return sorted(ids)
