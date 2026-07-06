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

/* Is the embedder usable this process? (LEGEND_EMBED_DIR set + model loads). */
int embed_available(void);

/* Rank elements (ids[i]/texts[i], i<n) by cosine of their embedding to `query`.
 * Fills out_ids/out_scores (desc, up to max); returns count, or -1 when the
 * embedder is unavailable (LEGEND_EMBED_DIR unset or model absent). Manages a
 * per-process model + a <LEGEND_STATE_DIR>/vectors.bin element-vector cache. */
int embed_rank_elements(const uint32_t *ids, const char *const *texts, int n,
                        const char *query, int qlen,
                        uint32_t *out_ids, float *out_scores, int max);

#endif
