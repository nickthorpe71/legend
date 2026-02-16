# Autonomous Dev Loop - Implementation Guide

This document contains the complete implementation plan for the autonomous feature development loop. Read this when starting implementation.

See the detailed plan that was approved in: **Claude's Plan.md** (from planning session)

## Quick Start for Tomorrow

When ready to implement:

1. **Start with Phase 1** - Core Infrastructure (3 hours)
   - Create PRD format and examples
   - Build PRD processor
   - Test PRD → Scenario generation

2. **Then Phase 2** - Loop Orchestrator (3 hours)
   - Build orchestrator
   - Add iteration tracking
   - Create CLI command

3. **Then Phase 3** - Agent Integration (2 hours)
   - Implement agent launchers
   - Connect agents to orchestrator

4. **Finally Phase 4** - Polish (2 hours)
   - Add stuck detection
   - Create beautiful reports
   - Write documentation

## Architecture Summary

**Hybrid approach**: Main Claude session orchestrates, specialized agents handle heavy lifting.

```
USER
  ↓
MAIN CLAUDE (Orchestrator)
  ├─→ PRD Agent (generate scenario)
  ├─→ Code Agent (implement feature)
  ├─→ Stickler (test)
  └─→ Fix Agent (fix failures)
```

## Key Components

1. **PRD Processor** (`src/prd/`)
   - Parse markdown PRDs
   - Generate Stickler scenarios
   - Validate completeness

2. **Loop Orchestrator** (`src/loop/orchestrator.ts`)
   - Main loop: generate → code → test → fix → repeat
   - Max 5 iterations default
   - Stuck detection (same error 3x)

3. **Agents** (`src/loop/agents.ts`)
   - PRD Agent: Analyze PRD, create scenario
   - Code Agent: Implement feature
   - Fix Agent: Analyze failure, apply fix

4. **Tracker** (`src/loop/tracker.ts`)
   - Record iterations
   - Detect patterns
   - Generate reports

## File Structure to Create

```
stickler/
├── prds/                    # NEW
│   ├── README.md           # PRD format guide
│   └── example-logout.md   # Example PRD
├── dev-loops/              # NEW (generated)
│   └── {timestamp}_{prd}/
├── src/
│   ├── prd/               # NEW
│   │   ├── processor.ts
│   │   ├── validator.ts
│   │   └── types.ts
│   └── loop/              # NEW
│       ├── orchestrator.ts
│       ├── agents.ts
│       ├── tracker.ts
│       └── reporter.ts
└── .ai-docs/
    └── dev-loop-guide.md  # NEW
```

## Expected Timeline

- **Day 1 Morning**: Phases 1-2 (PRD processing + orchestrator)
- **Day 1 Afternoon**: Phase 3 (agent integration)
- **Day 2**: Phase 4 (polish + docs)
- **Total**: ~10 hours

## Testing Strategy

After each phase:
- Phase 1: Manually test PRD → Scenario conversion
- Phase 2: Run loop with manual steps (no agents)
- Phase 3: Run fully autonomous loop
- Phase 4: Test complex features, edge cases

## Success Criteria

System should:
- ✅ Parse well-formed PRDs → valid scenarios (100%)
- ✅ Implement features autonomously (70%+ success on first try)
- ✅ Fix failures within 3 iterations (90%+ success)
- ✅ Detect stuck states intelligently
- ✅ Generate actionable reports

## Example Usage (After Implementation)

```bash
# Create PRD
cat > stickler/prds/add-logout.md << 'EOF'
# Feature: Add Logout Button
[... PRD content ...]
EOF

# Run autonomous development
npm run cli dev-loop add-logout --verbose

# Watch it:
# 1. Generate scenario from PRD
# 2. Implement the feature
# 3. Test with Stickler
# 4. Fix any failures
# 5. Repeat until pass
```

## Notes for Implementation

- Start simple, add complexity incrementally
- Test each component in isolation first
- Use verbose logging during development
- Keep agents' prompts focused and clear
- Don't over-engineer - get MVP working first

## Reference Materials

- **Full Plan**: See Claude's Plan.md from planning session
- **Stickler Docs**: README.md, stickler_plan.md
- **Agent Docs**: .ai-docs/ folder

---

**Ready to build tomorrow!** 🚀

Start with Phase 1 and work through incrementally.
