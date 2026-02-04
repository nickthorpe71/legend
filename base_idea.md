# 🧭 Legend — Project Context MCP

## 1. Core Idea

**Legend ** is a lightweight **context memory layer** that lives alongside your project — like a small local brain.  
It tracks:

- What features the project has  
- The progress on each feature  
- Which files are involved  
- What’s currently being worked on  

When a coding model or AI assistant starts working, it can **call Legend** to instantly understand the project — without reloading the entire repo or past chat logs.

> “The project’s state, serialized for an AI.”

---

## 2. What It Looks Like in a Project

When you run `legend init`, it creates a `.legend` directory in your repo root.  
This directory is automatically updated as you (or the LLM) work.

TODO: How do we make sure this stays up to date? Do we have instructions in CLAUDE.md? Or is there some other way we can have claude know to update? 

---

## 3. Workflow (Step-by-Step)

### Step 1 — LLM or human calls Legend

When a model begins working on a project, it calls `legend get_state`.  
Legend returns the project snapshot, including:

- The project’s purpose  
- Feature breakdown and status
- Attention map per feature (how often each dir/file/function is visited for each feature) 

The LLM can now **instantly load context** without reading all the code.

---

### Step 2 — Developer or model makes progress

While coding, changes happen (files are edited, bugs fixed, new feature added).

The LLM or CLI hook calls `legend update` with updates to status, file attention, and next steps.  
Legend merges that into `.legend`.  
It now reflects the **latest progress and focus**, so both human and model share the same state.

---

### Step 3 — When the model resumes later

The next day, when the model reopens the same repo and calls `legend get_state`,  
it instantly sees the current focus, relevant files, and next steps.  

The model can **continue exactly where it left off**, even without previous context.

---

### Step 4 — Human-readable summary

Developers can also check progress manually using `legend show`.  
It prints a readable summary of all features, their files, and next actions.

---

## 4. Why It’s Useful

| Problem | Legend’s Solution |
|----------|------------------|
| LLMs lose context between sessions | Stores persistent “mental map” of project |
| Models waste tokens re-parsing repos | Pre-summarizes active files + goals |
| Developers forget what’s done | Feature map acts as a living roadmap |
| Sync between human + AI is messy | Shared `.legend` keeps both in sync |

