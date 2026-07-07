/* Pure-C MiniLM sentence embedder — see embed.c. */
#ifndef LEGEND_EMBED_H
#define LEGEND_EMBED_H

#include <stdint.h>

typedef struct EmbedModel EmbedModel;

/* Load weights (minilm.f32.bin) + vocab (vocab.txt). NULL on failure. */
EmbedModel *embed_load(const char *bin_path, const char *vocab_path);
void embed_free(EmbedModel *m);

/* Embed `text` into out[384] (mean-pooled, L2-normalized). 0 on success. */
int embed_text(const EmbedModel *m, const char *text, float *out);

/* Tokenize into ids (incl [CLS]/[SEP]); returns token count. */
int embed_tokenize(const EmbedModel *m, const char *text, int *ids, int max);

/* Cheap: embedder enabled + weight blob present? (env + one stat, no load). */
int embed_enabled(void);

/* Is the embedder usable this process? (default model dir + LEGEND_EMBED!=0).
 * Loads the model on first call; embed_enabled() is the load-free predicate. */
int embed_available(void);

/* Load the model + vector sidecar up front (idempotent, safe no-op when
 * embeddings are off). A long-lived server calls this at startup so the first
 * request doesn't pay the load; one-shot CLI runs skip it and load lazily. */
void embed_warm(void);

/* Embed any new/changed elements (ids[i]/texts[i], i<n) into the sidecar cache
 * and persist. Call after a save so vectors are ready before the first recall. */
void embed_sync_elements(const uint32_t *ids, const char *const *texts, int n);

/* Rank elements (ids[i]/texts[i], i<n) by cosine of their embedding to `query`.
 * Fills out_ids/out_scores (desc, up to max); returns count, or -1 when the
 * embedder is unavailable (LEGEND_EMBED_DIR unset or model absent). Manages a
 * per-process model + a <LEGEND_STATE_DIR>/vectors.bin element-vector cache. */
int embed_rank_elements(const uint32_t *ids, const char *const *texts, int n,
                        const char *query, int qlen,
                        uint32_t *out_ids, float *out_scores, int max);

#endif
