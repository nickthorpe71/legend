"""OpenAI tool definitions mirroring the Legend save/recall payloads.

The descriptions carry the discipline from docs/mcp-server.md so the model
uses the primitives the way the store expects (recall before save, reuse
canonical names, `changes` for updates, few precise elements). `src` is
required on every fact: grounding is cheap to collect now and load-bearing for
the full study's contamination audit.
"""

LEGEND_SAVE_TOOL = {
    "type": "function",
    "function": {
        "name": "legend_save",
        "description": (
            "Write structured knowledge into the Legend graph. Before saving, "
            "call legend_recall to find whether an entity already exists and reuse "
            "its canonical name verbatim — never mint a second element for the same "
            "thing. Prefer a few precise elements and durable facts over many shallow "
            "ones; over-extraction buries the signal. To change a value that is now "
            "different, use `changes` (not a new fact). To remove something now false, "
            "use `retract`. To fold a duplicate you created, use `merge`."
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Short label for where this batch came from, e.g. the page title.",
                },
                "elements": {
                    "type": "array",
                    "description": "Durable named things to mint or reuse.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Canonical name; reuse an existing one exactly if it exists."},
                            "kind": {"type": "string", "description": "e.g. person, place, organization, work, event, concept."},
                            "summary": {"type": "string", "description": "One line describing the element."},
                            "aliases": {"type": "array", "items": {"type": "string"}, "description": "Other names/spellings for the same element."},
                        },
                        "required": ["name", "kind"],
                        "additionalProperties": False,
                    },
                },
                "facts": {
                    "type": "array",
                    "description": "Subject-property-object triples over elements.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "s": {"type": "string", "description": "Subject element (canonical name)."},
                            "p": {"type": "string", "description": "Property/relation name."},
                            "o": {"type": "string", "description": "Object value (an element name or a literal)."},
                            "confidence": {"type": "number", "description": "0..1 confidence this fact is correct."},
                            "src": {"type": "string", "description": "Where this fact came from — page id/title and locus. REQUIRED for grounding."},
                        },
                        "required": ["s", "p", "o", "src"],
                        "additionalProperties": False,
                    },
                },
                "changes": {
                    "type": "array",
                    "description": "Supersede a current value while keeping history.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "target": {"type": "string"},
                            "property": {"type": "string"},
                            "from": {"type": "string"},
                            "to": {"type": "string"},
                        },
                        "required": ["target", "property", "to"],
                        "additionalProperties": False,
                    },
                },
                "retract": {
                    "type": "array",
                    "description": "Remove facts that are now false.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "s": {"type": "string"},
                            "p": {"type": "string"},
                            "o": {"type": "string"},
                        },
                        "required": ["s", "p", "o"],
                        "additionalProperties": False,
                    },
                },
                "merge": {
                    "type": "array",
                    "description": "Fold a duplicate element into the canonical one.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "from": {"type": "string"},
                            "into": {"type": "string"},
                        },
                        "required": ["from", "into"],
                        "additionalProperties": False,
                    },
                },
            },
            "additionalProperties": False,
        },
    },
}

LEGEND_RECALL_TOOL = {
    "type": "function",
    "function": {
        "name": "legend_recall",
        "description": (
            "Resolve a focus against the Legend graph and return the focused subgraph "
            "(current state, decisions, constraints, history, related, sources). Use it "
            "to check what already exists before saving, and to look up facts when "
            "answering. Focus terms resolve through exact name, alias, lexical, then "
            "embedding matching, so paraphrases can still hit."
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "focus": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Entities/topics to resolve. Try the most specific names you can.",
                },
                "limit": {"type": "integer", "description": "Max related items per band."},
                "history_depth": {"type": "integer", "description": "How many superseded values to include per property."},
            },
            "required": ["focus"],
            "additionalProperties": False,
        },
    },
}

SAVE_TOOLS = [LEGEND_SAVE_TOOL, LEGEND_RECALL_TOOL]
RECALL_TOOLS = [LEGEND_RECALL_TOOL]
