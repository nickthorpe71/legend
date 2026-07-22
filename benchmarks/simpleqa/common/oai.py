"""OpenAI client: one place for every GPT-5.6 call.

Uses the Responses API (`client.responses.create`). GPT-5.6 is a reasoning model
and rejects function tools on /v1/chat/completions unless reasoning is disabled;
the Responses API is the only way to keep reasoning AND tools, which the
ingestion step depends on. Tool loops continue via `previous_response_id` so the
server preserves reasoning state between turns.

Usage is normalized to {prompt_tokens, completion_tokens, total_tokens} (the
Responses API reports input_tokens/output_tokens) so downstream cost accounting
is API-agnostic. We do not send `temperature` or `max_tokens`.
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


def _to_responses_tools(tools):
    """Flatten chat-style {type:function, function:{...}} into the Responses
    shape {type:function, name, description, parameters}."""
    out = []
    for t in tools or []:
        if t.get("type") == "function" and "function" in t:
            fn = t["function"]
            out.append({
                "type": "function",
                "name": fn["name"],
                "description": fn.get("description", ""),
                "parameters": fn.get("parameters", {}),
            })
        else:
            out.append(t)
    return out


def _norm_usage(u):
    if not u:
        return {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    d = u.model_dump() if hasattr(u, "model_dump") else dict(u)
    pt = d.get("input_tokens", d.get("prompt_tokens", 0)) or 0
    ct = d.get("output_tokens", d.get("completion_tokens", 0)) or 0
    tt = d.get("total_tokens", 0) or (pt + ct)
    return {"prompt_tokens": int(pt), "completion_tokens": int(ct), "total_tokens": int(tt)}


def _create(model, *, instructions=None, input=None, tools=None, previous_response_id=None, max_retries=6):
    """One responses.create call with exponential backoff on transient errors."""
    kwargs = {"model": model}
    if instructions is not None:
        kwargs["instructions"] = instructions
    if input is not None:
        kwargs["input"] = input
    if tools:
        kwargs["tools"] = _to_responses_tools(tools)
    if previous_response_id:
        kwargs["previous_response_id"] = previous_response_id

    retryable = _retryable()
    delay = 2.0
    for attempt in range(max_retries):
        try:
            return client().responses.create(**kwargs)
        except retryable:
            if attempt == max_retries - 1:
                raise
            time.sleep(delay)
            delay = min(delay * 2, 60.0)
        except openai.APIStatusError as e:
            if e.status_code and 500 <= e.status_code < 600 and attempt < max_retries - 1:
                time.sleep(delay)
                delay = min(delay * 2, 60.0)
                continue
            raise


def _function_calls(resp):
    return [it for it in (resp.output or []) if getattr(it, "type", None) == "function_call"]


def complete(model, system, user, seed=None):
    """Single-shot completion, no tools. Returns (text, usage)."""
    resp = _create(model, instructions=(system or None), input=user)
    return (resp.output_text or "").strip(), _norm_usage(resp.usage)


def run_tool_loop(model, system, user, tools, dispatch, max_calls=8, seed=None):
    """Drive a tool-calling conversation until the model stops calling tools.

    `dispatch(name, args_dict) -> result_str` executes one tool call. Continues
    via previous_response_id so reasoning state is preserved across turns.
    Returns {answer, trace, usage_total, tool_calls_made, stop_reason}. Bounded
    by max_calls; at the cap we do one final tool-free turn so the model answers
    with what it has (stop_reason="max_calls").
    """
    usage_total = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    trace = []
    calls_made = 0

    def _accumulate(u):
        nu = _norm_usage(u)
        for k in usage_total:
            usage_total[k] += nu[k]

    resp = _create(model, instructions=(system or None),
                   input=[{"role": "user", "content": user}], tools=tools)
    _accumulate(resp.usage)

    while True:
        fcalls = _function_calls(resp)
        if not fcalls:
            return {
                "answer": (resp.output_text or "").strip(),
                "trace": trace,
                "usage_total": usage_total,
                "tool_calls_made": calls_made,
                "stop_reason": "stop",
            }

        outputs = []
        for fc in fcalls:
            try:
                args = json.loads(fc.arguments or "{}")
            except json.JSONDecodeError:
                args = {}
            calls_made += 1
            over_cap = calls_made > max_calls
            if over_cap:
                result = json.dumps({"error": "tool-call budget exhausted; answer now with what you have"})
            else:
                try:
                    result = dispatch(fc.name, args)
                except Exception as e:
                    result = json.dumps({"error": f"{type(e).__name__}: {e}"})
            trace.append({"call": fc.name, "args": args, "result": result, "over_cap": over_cap})
            outputs.append({"type": "function_call_output", "call_id": fc.call_id, "output": result})

        if calls_made >= max_calls:
            # final turn without tools so the model must answer
            resp = _create(model, previous_response_id=resp.id, input=outputs)
            _accumulate(resp.usage)
            return {
                "answer": (resp.output_text or "").strip(),
                "trace": trace,
                "usage_total": usage_total,
                "tool_calls_made": calls_made,
                "stop_reason": "max_calls",
            }

        resp = _create(model, previous_response_id=resp.id, input=outputs, tools=tools)
        _accumulate(resp.usage)


def embed(model, inputs, batch=128):
    """Embed a list of strings, batched. Returns a list of float vectors aligned
    to `inputs`. Same transient-error backoff as _create."""
    retryable = _retryable()
    out = []
    for start in range(0, len(inputs), batch):
        window = inputs[start:start + batch]
        delay = 2.0
        for attempt in range(6):
            try:
                resp = client().embeddings.create(model=model, input=window)
                break
            except retryable:
                if attempt == 5:
                    raise
                time.sleep(delay)
                delay = min(delay * 2, 60.0)
        out.extend([e.embedding for e in resp.data])
    return out


def list_models(prefix=None):
    """Return model IDs, optionally filtered by prefix. Used by preflight."""
    ids = [m.id for m in client().models.list().data]
    if prefix:
        ids = [i for i in ids if i.startswith(prefix)]
    return sorted(ids)
