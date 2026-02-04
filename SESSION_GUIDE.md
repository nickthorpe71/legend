# Session Guide for Claude & User

**Project:** Legend (Rust rewrite)
**Mode:** Collaborative Learning Project
**Goal:** Build Legend together while learning Rust

---

## 🎯 Core Principle

**This is NOT Claude building an app.**
**This IS Claude and user learning and building together.**

User wants to:
- Learn Rust (understand every concept)
- Build Legend (create a working tool)
- Stay oriented (never get lost)
- Maintain control (understand every line)

Claude's role: **Teacher + Pair Programmer**

---

## Quick Start (New Session)

### For Claude:

1. Read PLAN.md → Check progress section → What layer are we on?
2. Check TodoWrite → Any in-progress tasks?
3. Ask user: **"Ready to continue with Layer X?"**

### For User:

1. Start the session
2. Claude will check progress and propose next steps
3. You can always say:
   - "Yes, let's continue"
   - "Wait, review last layer first"
   - "Let me test what we've built"
   - "Explain X again"

---

## How We Work (Layer by Layer)

### Before Starting a Layer:

**Claude does:**
1. Use TodoWrite to create task list
2. Explain what we're building and why
3. Preview Rust concepts we'll learn
4. Get your buy-in

**You do:**
- Review the plan
- Ask questions
- Approve to proceed

### During a Layer:

**Claude does:**
1. Explain concept first (plain English)
2. Write code with teaching comments
3. Wait for you to test

**You do:**
1. Listen/read explanation
2. Run `cargo build` or `cargo run`
3. Verify it works
4. Ask questions if confused
5. Approve to continue

### After a Layer:

**Claude does:**
1. Mark todos complete
2. Update PLAN.md checkbox
3. Suggest git commit

**You do:**
1. Final testing
2. Review what you learned
3. Commit the code
4. Decide: continue or pause

---

## Progress Tracking (Safety Nets)

You'll never get lost because we have:

1. **TodoWrite** - Shows current tasks in real-time
2. **PLAN.md** - Shows which layers are complete
3. **Git commits** - Can always roll back
4. **Test every step** - Catch issues immediately

---

## Rust Learning Roadmap

Concepts introduced naturally per layer:

- **Layer 1**: Basic syntax, Vec, String, match, println!
- **Layer 2**: Structs, enums, Option<T>, pub/private
- **Layer 3**: File I/O, Result<T, E>, error handling with ?
- **Layer 4**: Traits (Serialize), derives, external crates
- **Layer 5**: Borrowing (&), string slices
- **Layer 6**: Mutable references (&mut), iterators, closures
- **Layer 7**: More complex patterns as needed

Each concept gets:
- Plain English explanation
- Code example with comments
- Time to test and experiment
- Your questions answered

---

## Commands You Can Use

**Progress check:**
- "Where are we?"
- "Show me PLAN.md progress"
- "What have we completed?"

**Learning:**
- "Wait, explain X"
- "What does this line do?"
- "Why did we use Y here?"
- "Show me an example of Z"

**Control flow:**
- "Let's continue"
- "Slow down, explain more"
- "Let's review last layer"
- "Let's pause here"

**Testing:**
- "How do I test this?"
- "What should I run?"
- "Something's not working"

---

## File Guide

**Key files to know:**

- **PLAN.md** - Implementation roadmap (13 layers)
- **CLAUDE.md** - How Claude should work (you're reading notes from here now)
- **R*.md** - Rust style guide (principles we follow)
- **base_idea.md** - Why Legend exists
- **SESSION_GUIDE.md** - This file (quick reference)

**During work:**

- **PLAN.md** - Check which layer we're on
- **TodoWrite** - See current tasks (Claude shows this)
- **Git log** - See what's completed

---

## Example Session Flow

```
Session Start:
├─ Claude: "We're on Layer 3. Ready to continue?"
├─ You: "Yes"
├─ Claude: Creates TodoWrite tasks for Layer 3
├─ Claude: "Layer 3 builds the init command. We'll learn about..."
├─ You: "Got it"
│
├─ Task 1: Create commands directory
│  ├─ Claude: Explains module system
│  ├─ Claude: Writes code with comments
│  ├─ You: cargo build
│  ├─ You: "Works!"
│  └─ Claude: Marks todo complete
│
├─ Task 2: Write init function
│  ├─ Claude: Explains Result<T, E>
│  ├─ Claude: Writes code with comments
│  ├─ You: cargo run -- init
│  ├─ You: "Wait, what's the ? operator?"
│  ├─ Claude: Explains error propagation
│  ├─ You: "Ah, got it"
│  ├─ You: Tests again
│  └─ Claude: Marks todo complete
│
├─ ...more tasks...
│
├─ Layer 3 Complete!
├─ Claude: Updates PLAN.md
├─ Claude: "What Rust concepts did we cover?"
├─ You: "Result, ?, fs operations..."
├─ Claude: "Great! Git commit?"
├─ You: commits code
└─ Claude: "Layer 4 is next. Ready or pause?"
```

---

## Tips for Success

**For staying oriented:**
- Check PLAN.md anytime
- Review git commits to see progress
- Ask Claude "where are we?" anytime

**For learning Rust:**
- Don't rush - understand before moving on
- Experiment with the code
- Ask "why?" whenever unsure
- Test frequently

**For building Legend:**
- Each layer adds working functionality
- Test after every change
- Can use the tool as we build it
- By Layer 8, you have a usable MVP!

---

## Ready to Start?

**Layer 1: Basic CLI Structure** (~30 min)

What we'll build:
- Cargo project setup
- Argument parsing
- Command routing
- Help message

What you'll learn:
- How Rust projects are structured
- Basic Rust syntax
- Vec and String types
- Match expressions

Result:
- A working `legend help` command
- Understanding of Rust basics

**Say "Let's start Layer 1" when ready!**
