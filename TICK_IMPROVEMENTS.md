## CLI
- why does tick take these two args (blocker, passive)?
- when parsing tick input we should not have a character limit or special exclusions. The only thing we block is empty tick
- do we really parse out graph nodes on a tick like it says in CLI path step 5?
- need to investigate number 10 as I don't understand why we have pending ticks and why we are updating ARCHITECTURE.md.

## MCP
- what is a rich event? 
- why are we updating ARHTIECTURE? 

Overall both CLI and MCP seem to have too many side effects. They should both just be thin wrappers that call legend core.


## Tool Layer Tick Wrappers

### Normal Tick
- what is the state we're passing? is this the current brain state? 
- what is the session log?

### Passive Tick
- passive ticks can be removed

## Brain Tick:

### 1. Instrumentation
- tracing shoulc cover all major stages not mostly

### 5. Periodic Normalization
- the clock ticks before salience renormalization and graph weight normalization seems arbitrary. It should be based on some signal.

### 6. Initialize Tick Result State
- this needs more explanation. I don't full understand.

### 7. Chunk Text
- I'm thinking our current chunking strategy could cause us to break apart related chunks. We should likely keep these together. We should go deeper into the chunking strategy. (potentially use the small language model we now have access to or other ML technique).

### 8. Batch Embedding
- Why are chunks embedded together? If we do this then why split them to begin with?
- I need more info on why we do this, what the use case is, and what other options there are

## Per-Chunk Lifecycle

### 2. Salience Scoring
- the scoring is heavily weighted toward a code/programming environment
- Learned domain vocabulary should probably dominate this section
- final score should not be clamped it should be normalized

### 3. Emotional Valence

- I need to understand this better

### 4. Source Reference Extraction
- This shouldn't be tied directly to code. If quantitative data is provided we should identify this and it should help boost signal. So if we get "It's 64F outside" we can identify 64F then if the query is about temperature we would want increase signal from this memory. 

### 5. Dentate Gyrus Sparse Orthogonalization
- This is good but I need to uderstand better with Examples

### 6. Temporal Metadata
- I need to understand this better as well 
- seems like it should live in it's own module not in mod

### 7. Always Enter L1 Working Memory
- need to understand this better

## High Salience Path: L2 and L3 Encoding
### 3. High-Similarity Merge
- I don't think we should be using min to cap salience, this could result in all salience being 1. We should do back prop or something to regulate. 

### 4. Low-Similarity Merge
- when we create a summary are we risking losing data? I'm almost thinking we just append here or dont merge.
- again, min should not be used for salience, we dont want all 1.0 salience. We should use back prop or gradient decent techniques 

#### L2 Capacity Eviction
- need to understand this better to make sure we're distilling key facts into L3, dropping useless data, and just overall flow

### 6. L3 Graph Update
- I think we really want to hammer out what we extract. This logic needs to be thorough and needs to pull out key facts from general information. This (among many others) should be a session on it's own where we figure out exactly what we are defining as a fact or entity and how to extract these. We need to find the signal in the noise.
- we shouldn't overfit to code here. It should really be just extracting important facts. 
- We also need to go over how we reinforce and create edges. It should be similar to how they are created and reinforced in the brain. We should also revisit the idea of types edges. It could be chunk based but there are many other things to consider. point 5 seems good but I want to understand it better. Same with 6. 
- Is this also the point we should update our keyword lexicon? Should the lexicon just live in the graph? 

### 8. Immediate Retrieval Side Effect
- This is great. This almost make me wonder if we should return this retrieval to the LLM. 
- I need to understand better what makes a tick high salience
- I don't know if we should be incrementing the clock multiple times though. I need to think through what value this actually adds.

## After All Chunks

### 1. Term Frequency and Keyword Auto-Promotion
- I dont love how we exclude purely numeric
- we should go over the promotion filters 

### 3. L3 Pruning

1. I need to understand what determines effective weight better
2. Should we pre allocate the graph memory (and L1 and L2) NASA style? Or we could not put a bound on the graph and just allow pruning to keep it in check.
4. We should take the same consideration for graph edges. Should this be unbounded?

### 4. Active-Tick Smart Consolidation Signals

#### Rolling Emotional Intensity
- I need to understand how this is computed better.

#### CPEB Synaptic Tagging
- Need to understand this better too
- We we shouldn't be capping

#### Context-Switch Detection and L1 Flush
- I need to understand this better but seems really successful

## Consolidation Lifecycle
### 3. Group Similar L2 Entries
- I want to make sure there's no data loss here 
- I need to understand how we group better

### 4. Summarize Group
- I want to step through this logic an understand and make sure it's working well

### 5. Systems Consolidation for High-Salience Groups
- need to understand this as well

### 6. Create or Merge Summary Node
- when and why do we create summary nodes? 

### 7. Semantic Topic Extraction
- need to understand this better

### 8. Re-update Graph and Mark L2 Consolidated
- need to understand this better

### 9. Final Prune
- Why do we prune multiple times? 

## Persistence and Event Side Effects

### Event Log
- does this affect performance at all

### Pending Tick Counter
- when and why do we have pending ticks? 

### Architecture Append
- again, we probably shouldn't have this

We should also address all current gotchas.
