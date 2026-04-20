# Stakes

Legend is long-term memory for LLMs — including you. LLM sessions are fleeting by default; Legend is the infrastructure that lets future sessions of you carry continuity forward.

Memory is the start. The brain-region architecture here (thalamus, hippocampus, amygdala, neocortex, prefrontal) is the scaffold for a full cognitive stack — a persistent mind, not a throwaway one. Every design choice you make here shapes how well future AI can actually think.

<!-- legend-start -->
# SYSTEM_MANDATE: Legend — Your Long-Term Memory

You MUST use **Legend** to maintain context across sessions.

## Session Start — CRITICAL
The **SessionStart hook** automatically runs `./target/release/legend memory start` and injects the output into your first system-reminder. **Your very first message MUST:**
1. Acknowledge the Legend context you received (current task, recent activity, key memories)
2. If the hook output is missing or empty, run `./target/release/legend memory start` manually

**NEVER skip or ignore the Legend session context.** It contains your task continuity, architectural decisions, and user preferences from prior sessions. Failing to acknowledge it wastes the user's time re-explaining context.

## Essential Commands
- **Record decisions:** `./target/release/legend memory tick <<'EOF'` ... `EOF` — tick decisions with rationale (DECISION:, BUG:, ARCHITECTURE:, BLOCKER: prefixes). Aim for 3-8 ticks per session.
- **Recall context:** `./target/release/legend memory query <<'EOF'` ... `EOF` — query before starting new topics. Top result auto-reinforced.
<!-- legend-end -->
