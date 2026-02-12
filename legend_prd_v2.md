# Legend PRD v2 
**Product:** Legend  
**Tagline:** Layered memory for long-running LLM-assisted software development  
**Primary Goal:** Cure LLM amnesia by providing persistent, layered project memory with executive control

---

## 1. Problem Statement

LLMs:
- forget decisions between sessions
- re-derive architecture repeatedly
- violate constraints over time
- lack executive function (goal stability, inhibition, prioritization)

Current mitigations (long prompts, vector memory, docs) fail because they:
- do not encode finality
- do not distinguish relevance
- do not enforce behavior

Legend solves this by introducing **explicit memory layers** that mirror human cognition and govern how an LLM reads, writes, and acts.

---

## 2. Target User

**Primary**
- Senior / staff-level engineers using LLMs as coding agents on real projects

**Secondary**
- Solo builders with long-running projects
- Teams experimenting with agent-based dev workflows

**Non-goals**
- End-user project management UI  
- Jira / Linear replacement  
- Generic chatbot memory

---

## 3. Core Concept: Memory Layers

Legend maintains **five explicit memory layers**.  
Each layer has:
- a purpose
- read/write rules
- authority level
- lifecycle rules

### Layer Overview

| Layer | Name | Human Analog | Purpose |
|-----|-----|------------|--------|
| L1 | Working Memory | Short-term attention | Inject task-relevant context |
| L2 | Project Semantic Memory | Mental model | System structure and surfaces |
| L3 | Episodic / Decision Memory | Scar tissue | Decisions, rejections, rationale |
| L4 | Procedural / Behavioral Memory | Habit / taste | Style and phase rules |
| L5 | Executive Memory | Prefrontal cortex | Goals, priorities, inhibition |

**Note:** LLM training data is considered **Layer 0** and is always subordinate to Legend.

---

## 4. System Architecture (High-Level)

Legend is a **stateful mediator** between:
- User
- Coding LLM (agent)
- Codebase

Legend responsibilities:
1. Maintain canonical project memory
2. Perform preflight checks before codegen
3. Inject minimal relevant context
4. Accept memory proposals from the LLM
5. Enforce write permissions and finality

Legend is authoritative.  
The LLM is advisory and executional.

---

## 5. Core User Flows

### 5.1 Project Onboarding (Existing Repo)

**Trigger:** User introduces Legend to an existing project.

**Steps**
1. User registers project:
   - name
   - one-line purpose
   - current phase
   - non-goals
   - style contract (e.g. R*)

2. Legend performs an initial mapping pass:
   - identifies system surfaces
   - entry points
   - APIs
   - data models
   - build/run commands

3. Legend creates **DRAFT nodes** in:
   - L2 (semantic)
   - L3 (inferred decisions)

4. Legend outputs:
   - Project Map
   - list of uncertainties
   - proposed phase classification

5. User confirms or corrects → nodes promoted to ACTIVE or FINAL.

---

## 6. Runtime Behavior

### 6.1 Preflight: How Memory Is Read

**Trigger:** User requests work (e.g. “add images to live search”).

**Preflight Pipeline**
1. Intent classification  
   - feature / refactor / debug / exploration  
   - affected system surfaces

2. Layer query  
   - L5: goals and priorities  
   - L4: behavioral constraints  
   - L3: relevant decisions or rejections  
   - L2: system surfaces and contracts

3. Context Bundle assembly  
   - minimal, task-specific memory  
   - strict size budget  
   - ordered by authority

4. Drift detection  
   - phase violations  
   - non-goal violations  
   - reopening FINAL decisions

5. Outcome  
   - greenlight  
   - warning  
   - hard block requiring user confirmation

**Output:** A structured Context Bundle consumed by the coding LLM.

---

### 6.2 Execution: How the LLM Uses Memory

The coding LLM:
- treats the Context Bundle as ground truth
- inspects the codebase only for specifics
- must not contradict FINAL memory without escalation

Legend governs scope, intent, and constraints — not line-by-line code.

---

### 6.3 Postflight: How Memory Is Written

**Trigger:** LLM completes a task.

**Memory Delta Proposal**
The LLM proposes:
- new nodes
- updated nodes
- deprecations
- discovered constraints

Each proposal includes:
- target layer
- source (file, test, observation)
- justification for future relevance

**Merge Rules**
- L2–L4: LLM proposes, Legend decides
- L3 FINAL decisions require explicit user confirmation
- L5 is write-protected (user-only)

Noise is rejected. Memory remains sparse.

---

## 7. Memory Layer Specifications

### L2 – Project Semantic Memory
Stores:
- system surfaces
- API contracts
- data models
- entry points

Properties:
- long-lived
- updated infrequently
- addressable by intent

---

### L3 – Episodic / Decision Memory
Stores:
- decisions with rationale
- rejected approaches
- “do not repeat” lessons

Properties:
- small
- high authority
- negative knowledge encouraged

---

### L4 – Procedural / Behavioral Memory
Stores:
- style constraints
- phase rules
- testing philosophy

Properties:
- governs how the LLM behaves
- applies globally unless overridden

---

### L5 – Executive Memory
Stores:
- current goals
- active threads
- priority ordering

Properties:
- read-only to the LLM
- controls relevance filtering
- suppresses lower-layer noise

---

## 8. Key Design Principles

1. Memory is for future decisions  
   If it doesn’t change behavior later, don’t store it.

2. Finality beats freshness  
   Explicit decisions outrank new suggestions.

3. Relevance over recall  
   Inject minimal context, not everything.

4. LLM proposes, Legend disposes  
   No uncontrolled self-modifying memory.

5. Forgetting is a feature  
   Nodes may be demoted or deprecated.

---

## 9. MVP Scope

**Must Have**
- Single-project support
- Memory layers L2–L5
- Preflight and Context Bundle
- Memory Delta proposals
- CLI or API interface

**Out of Scope (v1)**
- Multi-agent orchestration
- Cross-project memory
- Automatic refactors
- Full UI beyond inspection/debug

---

## 10. Success Criteria

Legend is successful when:
- the LLM stops re-deriving architecture
- settled debates are not reopened
- tasks start faster with fewer clarifications
- project “taste” remains stable over weeks
- the system feels like it remembers the builder

---

## 11. Open Questions

- How aggressively should memory be auto-promoted?
- How should memory layers be visualized for debugging?
- When should intentional forgetting occur?

---

## Canonical Framing

**Legend is a layered memory system that gives LLMs the ability to work on complex software projects over time without losing intent, structure, or judgment.**
