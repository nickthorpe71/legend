# Legend: Vision & Architecture

> A layered memory system for LLM-assisted software development that cures AI amnesia.

## The Problem

Every time an LLM coding assistant starts a new session, it forgets everything:
architectural decisions, rejected approaches, project goals, style preferences,
what it just spent 3 hours building yesterday. Engineers repeat themselves endlessly,
the LLM re-proposes already-rejected ideas, and hard-won project knowledge evaporates.

## The Solution

Legend is a **stateful memory system** that sits between the developer, the LLM, and
the codebase. It provides persistent, layered memory modeled on human cognition —
not a database to query, but a **memory system that shapes behavior**.

Drop it into any project. Run `legend init`. The LLM gains persistent memory
automatically — no configuration, no forms, no setup wizards.

## Design Principles

1. **Zero-config by default.** `legend init` should work with no user input. Everything
   is auto-inferred from the codebase and progressively enriched by the LLM.
2. **Memory is for future decisions, not storage.** Every node must answer: "How does
   knowing this change what the LLM does next?"
3. **Finality beats freshness.** A battle-tested FINAL decision outranks any new proposal.
4. **LLM proposes, Legend disposes.** The LLM suggests memory updates; Legend enforces
   rules, permissions, and promotion gates.
5. **Forgetting is a feature.** Stale, unretrieved nodes decay and get pruned. Relevance
   over recall.
6. **Memory dynamics, not just storage.** Consolidation, priming, retrieval strengthening,
   and decay make this a *memory system*, not a filing cabinet.

## Performance Targets

| Operation | Target | Rationale |
|-----------|--------|-----------|
| `get-state` | <5ms | LLM reads on every prompt |
| `preflight` | <50ms | Runs before every LLM response |
| `propose` | <500ms | Write path, less latency-sensitive |
| `consolidate` | <2s | Runs post-session, not blocking |
| State file size | <1MB | Even for large projects with 500+ nodes |
