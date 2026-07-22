/* Pure-C all-MiniLM-L6-v2 sentence embedder. In-process, no daemon, no Python.
 *
 * (feature macro: strdup/getline are POSIX.1-2008, not in strict c99)
 */
#define _POSIX_C_SOURCE 200809L
/*
 * Reads two committed assets produced once by tools/embed_prep (offline, Rust):
 *   minilm.int8.bin  weights (per-row int8 + fp32 small) in embed.c's fixed order
 *   vocab.txt       id-ordered WordPiece vocab (line N == token id N)
 *
 * embed_text(m, text, out384) = BERT-uncased WordPiece tokenize -> 6-layer
 * encoder -> mean-pool over tokens -> L2 normalize. Deterministic fp32 math,
 * libm only. Build with legend: gcc ... legend.c embed.c -lm
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <stdint.h>
#include <sys/stat.h>

#include "embed.h"

#if defined(__x86_64__) || defined(__i386__)
#include <immintrin.h>
#define EMBED_X86 1
#else
#define EMBED_X86 0
#endif

/* ---- fixed architecture (all-MiniLM-L6-v2) ---- */
enum { HID = 384, NL = 12, NH = 12, DH = HID / NH, INTER = 1536,
       NVOCAB = 30522, MAXPOS = 512, TYPE_ID = 0 };
static const double LN_EPS = 1e-12;

typedef struct {
    float *qw, *qb, *kw, *kb, *vw, *vb;   /* proj: w [HID*HID] row-major [out][in], b [HID] */
    float *aow, *aob;                     /* attn output dense: w [HID*HID], b [HID] */
    float *alg, *alb;                     /* attn LayerNorm gamma/beta [HID] */
    float *iw, *ib;                       /* intermediate: w [INTER*HID], b [INTER] */
    float *ow, *ob;                       /* output dense: w [HID*INTER], b [HID] */
    float *olg, *olb;                     /* output LayerNorm gamma/beta [HID] */
} Layer;

/* open-addressing vocab hash: token string -> id */
typedef struct {
    char **key;   /* owned strings, NULL = empty slot */
    int *val;     /* token id */
    uint32_t cap; /* power of two */
} Vocab;

struct EmbedModel {
    float *word_emb;              /* [NVOCAB*HID] */
    float *pos_emb;               /* [MAXPOS*HID] */
    float *type_emb;              /* [2*HID] */
    float *emb_lg, *emb_lb;       /* embeddings LayerNorm [HID] */
    Layer layer[NL];
    Vocab vocab;
    int cls, sep, unk;           /* special ids */
};

/* ============================ vocab hash ============================ */

static uint32_t fnv1a(const char *s) {
    uint32_t h = 2166136261u;
    for (; *s; s++) { h ^= (unsigned char)*s; h *= 16777619u; }
    return h;
}

static void vocab_put(Vocab *v, const char *key, int id) {
    uint32_t i = fnv1a(key) & (v->cap - 1);
    while (v->key[i]) {
        if (strcmp(v->key[i], key) == 0) { v->val[i] = id; return; }
        i = (i + 1) & (v->cap - 1);
    }
    v->key[i] = strdup(key);
    v->val[i] = id;
}

/* -1 if absent */
static int vocab_get(const Vocab *v, const char *key, size_t len) {
    char buf[512];
    if (len >= sizeof buf) return -1;
    memcpy(buf, key, len); buf[len] = 0;
    uint32_t i = fnv1a(buf) & (v->cap - 1);
    while (v->key[i]) {
        if (strcmp(v->key[i], buf) == 0) return v->val[i];
        i = (i + 1) & (v->cap - 1);
    }
    return -1;
}

/* ============================ tokenizer ============================ */
/* BERT uncased: clean_text + lowercase (ASCII; UTF-8 passed through) ->
 * split on whitespace + isolate punctuation -> greedy '##' WordPiece.
 * Unicode accent-strip / CJK spacing are TODO (absent from the ASCII corpus). */

static int is_ws(unsigned c) { return c == ' ' || c == '\t' || c == '\n' ||
                                      c == '\r' || c == '\f' || c == '\v'; }
static int is_ctrl(unsigned c) { return (c < 0x20 && !is_ws(c)) || c == 0x7f; }
static int is_ascii_punct(unsigned c) {
    return (c >= 33 && c <= 47) || (c >= 58 && c <= 64) ||
           (c >= 91 && c <= 96) || (c >= 123 && c <= 126);
}

/* Append one WordPiece-tokenized word (NUL-terminated, already normalized). */
static void wordpiece(const EmbedModel *m, const char *w, int *ids, int *n, int max) {
    size_t len = strlen(w);
    if (len == 0) return;
    if (len > 100) { if (*n < max) ids[(*n)++] = m->unk; return; }
    size_t start = 0;
    int pieces[128], np = 0;
    int bad = 0;
    while (start < len) {
        size_t end = len;
        int cur = -1;
        while (start < end) {
            char sub[128];
            size_t sl = 0;
            if (start > 0) { sub[sl++] = '#'; sub[sl++] = '#'; }
            memcpy(sub + sl, w + start, end - start); sl += end - start;
            int id = vocab_get(&m->vocab, sub, sl);
            if (id >= 0) { cur = id; break; }
            end--; /* NOTE: byte-level; ASCII corpus so == char-level */
        }
        if (cur < 0) { bad = 1; break; }
        if (np < 128) pieces[np++] = cur;
        start = end;
    }
    if (bad) { if (*n < max) ids[(*n)++] = m->unk; return; }
    for (int i = 0; i < np && *n < max; i++) ids[(*n)++] = pieces[i];
}

/* Tokenize `text` into ids incl [CLS]/[SEP]. Returns token count. */
int embed_tokenize(const EmbedModel *m, const char *text, int *ids, int max) {
    int n = 0;
    if (n < max) ids[n++] = m->cls;
    char word[256];
    size_t wl = 0;
    #define FLUSH() do { if (wl) { word[wl] = 0; wordpiece(m, word, ids, &n, max); wl = 0; } } while (0)
    for (const unsigned char *p = (const unsigned char *)text; *p; p++) {
        unsigned c = *p;
        if (is_ctrl(c)) continue;                 /* clean_text: drop controls */
        if (is_ws(c)) { FLUSH(); continue; }       /* whitespace splits words */
        if (c < 0x80 && is_ascii_punct(c)) {        /* punctuation is its own token */
            FLUSH();
            char pc[2] = { (char)c, 0 };
            wordpiece(m, pc, ids, &n, max);
            continue;
        }
        if (c < 0x80 && c >= 'A' && c <= 'Z') c += 32; /* lowercase ASCII */
        if (wl + 1 < sizeof word) word[wl++] = (char)c;
    }
    FLUSH();
    #undef FLUSH
    if (n < max) ids[n++] = m->sep;
    return n;
}

/* ============================ math ============================ */

#if EMBED_X86 && !defined(EMBED_NO_SIMD)
/* AVX2+FMA dot product, float accumulate (2 lanes). The target attribute
 * compiles this for AVX2/FMA even under the default -O2 (no global -mavx2);
 * a runtime __builtin_cpu_supports gate keeps pre-AVX2 CPUs on the scalar path.
 * float accumulate matches the f32 reference within the cos>0.999 gate. */
__attribute__((target("avx2,fma")))
static float dot_avx2(const float *a, const float *b, int n) {
    __m256 a0 = _mm256_setzero_ps(), a1 = _mm256_setzero_ps();
    int i = 0;
    for (; i + 16 <= n; i += 16) {
        a0 = _mm256_fmadd_ps(_mm256_loadu_ps(a + i), _mm256_loadu_ps(b + i), a0);
        a1 = _mm256_fmadd_ps(_mm256_loadu_ps(a + i + 8), _mm256_loadu_ps(b + i + 8), a1);
    }
    for (; i + 8 <= n; i += 8)
        a0 = _mm256_fmadd_ps(_mm256_loadu_ps(a + i), _mm256_loadu_ps(b + i), a0);
    __m256 s8 = _mm256_add_ps(a0, a1);
    __m128 s4 = _mm_add_ps(_mm256_castps256_ps128(s8), _mm256_extractf128_ps(s8, 1));
    s4 = _mm_hadd_ps(s4, s4);
    s4 = _mm_hadd_ps(s4, s4);
    float acc = _mm_cvtss_f32(s4);
    for (; i < n; i++) acc += a[i] * b[i];
    return acc;
}

static int embed_have_avx2(void) {
    static int v = -1;
    if (v < 0) v = __builtin_cpu_supports("avx2") && __builtin_cpu_supports("fma");
    return v;
}
#endif

/* y[o] = sum_i x[i]*W[o*in+i] + b[o], W row-major [out][in]. */
static void linear(const float *x, const float *W, const float *b,
                   float *y, int out, int in) {
#if EMBED_X86 && !defined(EMBED_NO_SIMD)
    if (embed_have_avx2()) {
        for (int o = 0; o < out; o++)
            y[o] = (b ? b[o] : 0.0f) + dot_avx2(x, W + (size_t)o * in, in);
        return;
    }
#endif
    for (int o = 0; o < out; o++) {
        const float *wr = W + (size_t)o * in;
        double acc = b ? b[o] : 0.0;
        for (int i = 0; i < in; i++) acc += (double)x[i] * wr[i];
        y[o] = (float)acc;
    }
}

/* in-place LayerNorm over each row of x[n][HID] */
static void layernorm(float *x, int n, const float *g, const float *b) {
    for (int t = 0; t < n; t++) {
        float *r = x + (size_t)t * HID;
        double mean = 0.0;
        for (int h = 0; h < HID; h++) mean += r[h];
        mean /= HID;
        double var = 0.0;
        for (int h = 0; h < HID; h++) { double d = r[h] - mean; var += d * d; }
        var /= HID;
        double inv = 1.0 / sqrt(var + LN_EPS);
        for (int h = 0; h < HID; h++)
            r[h] = (float)(((r[h] - mean) * inv) * g[h] + b[h]);
    }
}

static float gelu(float x) { /* exact erf gelu (BERT default hidden_act="gelu") */
    return (float)(0.5 * x * (1.0 + erf((double)x / 1.4142135623730951)));
}

/* ============================ forward ============================ */

int embed_text(const EmbedModel *m, const char *text, float *out) {
    int ids[MAXPOS];
    int n = embed_tokenize(m, text, ids, MAXPOS);
    if (n <= 0) return -1;

    /* one allocation for the whole forward pass */
    size_t nh = (size_t)n * HID;
    float *buf = malloc((6 * nh + (size_t)n * INTER + (size_t)n) * sizeof *buf);
    if (!buf) return -1;
    float *x = buf, *Q = x + nh, *K = Q + nh, *V = K + nh, *ctx = V + nh,
          *tmp = ctx + nh, *inter = tmp + nh, *scores = inter + (size_t)n * INTER;
    /* embeddings = word + pos + type, then LayerNorm */
    for (int t = 0; t < n; t++) {
        const float *we = m->word_emb + (size_t)ids[t] * HID;
        const float *pe = m->pos_emb + (size_t)t * HID;
        const float *te = m->type_emb + (size_t)TYPE_ID * HID;
        float *r = x + (size_t)t * HID;
        for (int h = 0; h < HID; h++) r[h] = we[h] + pe[h] + te[h];
    }
    layernorm(x, n, m->emb_lg, m->emb_lb);

    for (int l = 0; l < NL; l++) {
        const Layer *L = &m->layer[l];
        for (int t = 0; t < n; t++) {
            linear(x + (size_t)t * HID, L->qw, L->qb, Q + (size_t)t * HID, HID, HID);
            linear(x + (size_t)t * HID, L->kw, L->kb, K + (size_t)t * HID, HID, HID);
            linear(x + (size_t)t * HID, L->vw, L->vb, V + (size_t)t * HID, HID, HID);
        }
        /* multi-head attention */
        double scale = 1.0 / sqrt((double)DH);
        for (int h = 0; h < NH; h++) {
            int off = h * DH;
            for (int i = 0; i < n; i++) {
                double mx = -1e30;
                for (int j = 0; j < n; j++) {
                    double dot = 0.0;
                    const float *qi = Q + (size_t)i * HID + off;
                    const float *kj = K + (size_t)j * HID + off;
                    for (int d = 0; d < DH; d++) dot += (double)qi[d] * kj[d];
                    dot *= scale;
                    scores[j] = (float)dot;
                    if (dot > mx) mx = dot;
                }
                double sum = 0.0;
                for (int j = 0; j < n; j++) { double e = exp(scores[j] - mx); scores[j] = (float)e; sum += e; }
                float *ci = ctx + (size_t)i * HID + off;
                for (int d = 0; d < DH; d++) ci[d] = 0.0f;
                for (int j = 0; j < n; j++) {
                    double p = scores[j] / sum;
                    const float *vj = V + (size_t)j * HID + off;
                    for (int d = 0; d < DH; d++) ci[d] += (float)(p * vj[d]);
                }
            }
        }
        /* attn output dense + residual + LayerNorm */
        for (int t = 0; t < n; t++)
            linear(ctx + (size_t)t * HID, L->aow, L->aob, tmp + (size_t)t * HID, HID, HID);
        for (size_t k = 0; k < (size_t)n * HID; k++) x[k] += tmp[k];
        layernorm(x, n, L->alg, L->alb);
        /* FFN: intermediate(gelu) -> output dense + residual + LayerNorm */
        for (int t = 0; t < n; t++) {
            linear(x + (size_t)t * HID, L->iw, L->ib, inter + (size_t)t * INTER, INTER, HID);
            for (int i = 0; i < INTER; i++) inter[(size_t)t * INTER + i] = gelu(inter[(size_t)t * INTER + i]);
            linear(inter + (size_t)t * INTER, L->ow, L->ob, tmp + (size_t)t * HID, HID, INTER);
        }
        for (size_t k = 0; k < (size_t)n * HID; k++) x[k] += tmp[k];
        layernorm(x, n, L->olg, L->olb);
    }

    /* CLS-pool: the [CLS] token (row 0) is the sentence vector, then L2 normalize */
    double nrm = 0.0;
    for (int h = 0; h < HID; h++) { out[h] = x[h]; nrm += (double)out[h] * out[h]; }
    nrm = sqrt(nrm);
    if (nrm > 0) for (int h = 0; h < HID; h++) out[h] = (float)(out[h] / nrm);

    free(buf);
    return 0;
}

/* ============================ loader ============================ */

/* Read one tensor (rows x cols): u8 flag; 0 = raw f32, 1 = per-row int8
 * (rows f32 scales + rows*cols int8, dequantized here). Fresh f32 array. */
static float *rd_tensor(FILE *f, size_t rows, size_t cols) {
    size_t n = rows * cols;
    unsigned char q;
    if (fread(&q, 1, 1, f) != 1) return NULL;
    float *p = malloc(n * sizeof *p);
    if (!p) return NULL;
    if (q == 0) {
        if (fread(p, sizeof *p, n, f) != n) { free(p); return NULL; }
    } else {
        float *scales = malloc(rows * sizeof *scales);
        signed char *qb = malloc(n);
        if (!scales || !qb || fread(scales, sizeof *scales, rows, f) != rows ||
            fread(qb, 1, n, f) != n) { free(scales); free(qb); free(p); return NULL; }
        for (size_t r = 0; r < rows; r++)
            for (size_t c = 0; c < cols; c++)
                p[r * cols + c] = (float)qb[r * cols + c] * scales[r];
        free(scales); free(qb);
    }
    return p;
}

EmbedModel *embed_load(const char *bin_path, const char *vocab_path) {
    EmbedModel *m = calloc(1, sizeof *m);
    if (!m) return NULL;

    /* ---- vocab.txt ---- */
    FILE *vf = fopen(vocab_path, "r");
    if (!vf) { free(m); return NULL; }
    m->vocab.cap = 65536;
    m->vocab.key = calloc(m->vocab.cap, sizeof *m->vocab.key);
    m->vocab.val = calloc(m->vocab.cap, sizeof *m->vocab.val);
    char line[256];
    int id = 0;
    while (id < NVOCAB && fgets(line, sizeof line, vf)) {
        size_t l = strlen(line);
        while (l && (line[l-1] == '\n' || line[l-1] == '\r')) line[--l] = 0;
        vocab_put(&m->vocab, line, id);
        id++;
    }
    fclose(vf);
    m->cls = vocab_get(&m->vocab, "[CLS]", 5);
    m->sep = vocab_get(&m->vocab, "[SEP]", 5);
    m->unk = vocab_get(&m->vocab, "[UNK]", 5);
    if (m->cls < 0 || m->sep < 0 || m->unk < 0) { embed_free(m); return NULL; }

    /* ---- minilm.int8.bin ---- */
    FILE *bf = fopen(bin_path, "rb");
    if (!bf) { embed_free(m); return NULL; }
    char magic[8];
    uint32_t hdr[6];
    if (fread(magic, 1, 8, bf) != 8 || memcmp(magic, "MINILM02", 8) != 0 ||
        fread(hdr, sizeof(uint32_t), 6, bf) != 6) { fclose(bf); embed_free(m); return NULL; }
    if (hdr[0] != HID || hdr[1] != NL || hdr[2] != NH || hdr[3] != INTER ||
        hdr[4] != NVOCAB || hdr[5] != MAXPOS) { fclose(bf); embed_free(m); return NULL; }

    int ok = 1;
    /* (rows, cols) must match prep's per-row split for the int8 tensors. */
    #define RD(dst, rows, cols) do { if (ok && !((dst) = rd_tensor(bf, (rows), (cols)))) ok = 0; } while (0)
    RD(m->word_emb, NVOCAB, HID);
    RD(m->pos_emb, MAXPOS, HID);
    RD(m->type_emb, 2, HID);
    RD(m->emb_lg, HID, 1); RD(m->emb_lb, HID, 1);
    for (int l = 0; l < NL; l++) {
        Layer *L = &m->layer[l];
        RD(L->qw, HID, HID); RD(L->qb, HID, 1);
        RD(L->kw, HID, HID); RD(L->kb, HID, 1);
        RD(L->vw, HID, HID); RD(L->vb, HID, 1);
        RD(L->aow, HID, HID); RD(L->aob, HID, 1);
        RD(L->alg, HID, 1); RD(L->alb, HID, 1);
        RD(L->iw, INTER, HID); RD(L->ib, INTER, 1);
        RD(L->ow, HID, INTER); RD(L->ob, HID, 1);
        RD(L->olg, HID, 1); RD(L->olb, HID, 1);
    }
    #undef RD
    fclose(bf);
    if (!ok) { embed_free(m); return NULL; }
    return m;
}

void embed_free(EmbedModel *m) {
    if (!m) return;
    for (uint32_t i = 0; i < m->vocab.cap; i++) free(m->vocab.key ? m->vocab.key[i] : NULL);
    free(m->vocab.key); free(m->vocab.val);
    free(m->word_emb); free(m->pos_emb); free(m->type_emb); free(m->emb_lg); free(m->emb_lb);
    for (int l = 0; l < NL; l++) {
        Layer *L = &m->layer[l];
        free(L->qw); free(L->qb); free(L->kw); free(L->kb); free(L->vw); free(L->vb);
        free(L->aow); free(L->aob); free(L->alg); free(L->alb);
        free(L->iw); free(L->ib); free(L->ow); free(L->ob); free(L->olg); free(L->olb);
    }
    free(m);
}

/* ===================== element vector cache + semantic rank =====================
 * Sidecar <store>/vectors.bin caches each element's embedding (keyed by id +
 * text hash) so we embed elements once, not once per query. Query embed is the
 * only per-recall model cost (~62ms); cosine over the cache is microseconds.
 * Enabled only when LEGEND_EMBED_DIR points at the model assets; otherwise the
 * caller falls back to the salience backfill (keeps the pure build model-free). */

static uint64_t fnv64(const char *s, int len) {
    uint64_t h = 1469598103934665603ULL;
    for (int i = 0; i < len; i++) { h ^= (unsigned char)s[i]; h *= 1099511628211ULL; }
    return h;
}

typedef struct { uint32_t id; uint64_t hash; float vec[HID]; } VecEnt;
static struct {
    EmbedModel *model;
    int tried;                /* model load attempted this process */
    int probed, usable;       /* config resolved (cheap): enabled + blob present */
    VecEnt *ent; int n, cap;  /* vector cache */
    int dirty, loaded;
    uint64_t blob_sig;        /* blob size^mtime: invalidates a stale sidecar */
    char binpath[1200], vocpath[1200], sidecar[1200];
} EC;

/* LEGEND_EMBED_TRACE!=0 -> one stderr line per embedding event (model load /
 * recall rank / save sync). Diagnostic only; off by default, never on stdout. */
static int ec_traced(void) {
    static int on = -1;
    if (on < 0) { const char *t = getenv("LEGEND_EMBED_TRACE"); on = (t && strcmp(t, "0") != 0); }
    return on;
}

/* Resolve config once (cheap: env + one stat, no model load): is the embedder
 * enabled (LEGEND_EMBED != 0) and the weight blob present? Records blob_sig so
 * the sidecar can be validated — and staleness checked — without the load. */
static int ec_probe(void) {
    if (EC.probed) return EC.usable;
    EC.probed = 1;
    const char *off = getenv("LEGEND_EMBED");
    if (off && strcmp(off, "0") == 0) return 0; /* explicit disable (fuzz/core gates) */
    const char *dir = getenv("LEGEND_EMBED_DIR");
    if (!dir || !*dir) dir = "models/bge-small-en-v1.5"; /* default: committed asset */
    snprintf(EC.binpath, sizeof EC.binpath, "%s/minilm.int8.bin", dir);
    snprintf(EC.vocpath, sizeof EC.vocpath, "%s/vocab.txt", dir);
    struct stat st;
    if (stat(EC.binpath, &st) != 0) return 0; /* no blob: graceful degrade */
    EC.blob_sig = (uint64_t)st.st_size ^ ((uint64_t)st.st_mtime << 1);
    EC.usable = 1;
    return 1;
}

static void ec_load_model(void) {
    if (EC.tried) return;
    EC.tried = 1;
    if (!ec_probe()) return;
    EC.model = embed_load(EC.binpath, EC.vocpath);
    if (EC.model && ec_traced()) fprintf(stderr, "[embed] model loaded\n");
}

static void ec_load_sidecar(void) {
    if (EC.loaded) return;
    EC.loaded = 1;
    const char *sd = getenv("LEGEND_STATE_DIR");
    snprintf(EC.sidecar, sizeof EC.sidecar, "%s/vectors.bin", (sd && *sd) ? sd : ".");
    FILE *f = fopen(EC.sidecar, "rb");
    if (!f) return;
    char magic[8]; uint32_t cnt; uint64_t sig;
    if (fread(magic, 1, 8, f) == 8 && memcmp(magic, "LGVEC002", 8) == 0 &&
        fread(&sig, 8, 1, f) == 1 && sig == EC.blob_sig &&  /* stale on model change */
        fread(&cnt, 4, 1, f) == 1 && cnt < (1u << 24)) {
        EC.ent = malloc((size_t)cnt * sizeof *EC.ent);
        if (EC.ent && fread(EC.ent, sizeof *EC.ent, cnt, f) == cnt) { EC.n = (int)cnt; EC.cap = (int)cnt; }
        else { free(EC.ent); EC.ent = NULL; }
    }
    fclose(f);
}

static const float *ec_vec_for(uint32_t id, const char *text) {
    uint64_t h = fnv64(text, (int)strlen(text));
    VecEnt *e = NULL;
    for (int i = 0; i < EC.n; i++) if (EC.ent[i].id == id) { e = &EC.ent[i]; break; }
    if (e && e->hash == h) return e->vec;
    float v[HID];
    if (embed_text(EC.model, text, v) != 0) return NULL;
    if (!e) {
        if (EC.n == EC.cap) {
            int nc = EC.cap ? EC.cap * 2 : 64;
            VecEnt *ne = realloc(EC.ent, (size_t)nc * sizeof *ne);
            if (!ne) return NULL;
            EC.ent = ne; EC.cap = nc;
        }
        e = &EC.ent[EC.n++]; e->id = id;
    }
    e->hash = h; memcpy(e->vec, v, sizeof v); EC.dirty = 1;
    return e->vec;
}

static void ec_persist(void) {
    if (!EC.dirty) return;
    FILE *f = fopen(EC.sidecar, "wb");
    if (!f) return;
    uint32_t cnt = (uint32_t)EC.n;
    if (fwrite("LGVEC002", 1, 8, f) == 8 && fwrite(&EC.blob_sig, 8, 1, f) == 1 &&
        fwrite(&cnt, 4, 1, f) == 1)
        fwrite(EC.ent, sizeof *EC.ent, cnt, f);
    fclose(f);
    EC.dirty = 0;
}

typedef struct { uint32_t id; float s; } RankCand;
static int rankcand_cmp(const void *a, const void *b) {
    const RankCand *x = a, *y = b;
    if (x->s > y->s) return -1;
    if (x->s < y->s) return 1;
    return x->id < y->id ? -1 : x->id > y->id ? 1 : 0;
}

/* Cheap gate: embedder enabled + blob present? (env + one stat, no model load).
 * Lets the save-time sync skip building an element list — and skip the 23 MB
 * load — when embeddings are off. */
int embed_enabled(void) {
    return ec_probe();
}

/* Is the model in RAM? (attempts the load once, cached). Implies embed_enabled;
 * callers that must embed a fresh query use this. */
int embed_available(void) {
    ec_load_model();
    return EC.model != NULL;
}

void embed_warm(void) {
    if (!ec_probe()) return; /* embeddings off: nothing to warm */
    ec_load_model();
    ec_load_sidecar();
}

/* Is (id,text) missing or stale in the sidecar? (hash check, no model). */
static int ec_stale(uint32_t id, const char *text) {
    uint64_t h = fnv64(text, (int)strlen(text));
    for (int i = 0; i < EC.n; i++)
        if (EC.ent[i].id == id) return EC.ent[i].hash != h;
    return 1;
}

/* Eager sync: ensure every element (ids[i]/texts[i]) has an up-to-date vector
 * in the sidecar, then persist. Embeds only new/changed elements, and loads the
 * model only when at least one needs it — so an idempotent save stays cheap and
 * the cost of real changes amortizes across saves instead of one lazy build. */
void embed_sync_elements(const uint32_t *ids, const char *const *texts, int n) {
    int i, work = 0;
    if (!ec_probe()) return;
    ec_load_sidecar();
    for (i = 0; i < n; i++)
        if (ec_stale(ids[i], texts[i])) work++;
    if (!work) return; /* fully cached: no model load, no rewrite */
    ec_load_model();
    if (!EC.model) return;
    for (i = 0; i < n; i++) ec_vec_for(ids[i], texts[i]);
    ec_persist();
    if (ec_traced())
        fprintf(stderr, "[embed] sync: embedded %d new/changed of %d elements\n", work, n);
}

/* Rank elements (ids[i]/texts[i]) by cosine to `query`. Fills out_ids/out_scores
 * (desc), returns count, or -1 if embeddings are unavailable. */
int embed_rank_elements(const uint32_t *ids, const char *const *texts, int n,
                        const char *query, int qlen,
                        uint32_t *out_ids, float *out_scores, int max) {
    ec_load_model();
    if (!EC.model) return -1;
    ec_load_sidecar();

    /* BGE asymmetric retrieval: the query carries a search instruction; the
     * documents (element vectors) are embedded bare. */
    static const char QPREFIX[] = "Represent this sentence for searching relevant passages: ";
    const int pl = (int)sizeof QPREFIX - 1;
    char qbuf[2048];
    if (qlen < 0) qlen = 0;
    if (qlen >= (int)sizeof qbuf - pl - 1) qlen = (int)sizeof qbuf - pl - 1;
    memcpy(qbuf, QPREFIX, pl);
    memcpy(qbuf + pl, query, qlen); qbuf[pl + qlen] = 0;
    float qv[HID];
    if (embed_text(EC.model, qbuf, qv) != 0) return -1;

    RankCand *c = malloc((size_t)n * sizeof *c);
    if (!c) return -1;
    int m = 0;
    for (int i = 0; i < n; i++) {
        const float *v = ec_vec_for(ids[i], texts[i]);
        if (!v) continue;
        double d = 0.0;                        /* qv, v both L2-normalized -> dot == cosine */
        for (int k = 0; k < HID; k++) d += (double)qv[k] * v[k];
        c[m].id = ids[i]; c[m].s = (float)d; m++;
    }
    ec_persist();
    if (ec_traced())
        fprintf(stderr, "[embed] recall: embedded query, cosine-ranked %d candidates\n", m);
    qsort(c, (size_t)m, sizeof *c, rankcand_cmp);
    int r = m < max ? m : max;
    for (int i = 0; i < r; i++) { out_ids[i] = c[i].id; out_scores[i] = c[i].s; }
    free(c);
    return r;
}

int embed_rank_texts(const char *const *texts, int n, const char *query,
                     int qlen, int *out_order, int max) {
    ec_load_model();
    if (!EC.model) return -1;

    static const char QPREFIX[] = "Represent this sentence for searching relevant passages: ";
    const int pl = (int)sizeof QPREFIX - 1;
    char qbuf[2048];
    if (qlen < 0) qlen = 0;
    if (qlen >= (int)sizeof qbuf - pl - 1) qlen = (int)sizeof qbuf - pl - 1;
    memcpy(qbuf, QPREFIX, pl);
    memcpy(qbuf + pl, query, qlen); qbuf[pl + qlen] = 0;
    float qv[HID];
    if (embed_text(EC.model, qbuf, qv) != 0) return -1;

    RankCand *c = malloc((size_t)n * sizeof *c);
    if (!c) return -1;
    int m = 0;
    for (int i = 0; i < n; i++) {
        float tv[HID];
        if (!texts[i] || !texts[i][0] || embed_text(EC.model, texts[i], tv) != 0) continue;
        double d = 0.0;                        /* qv, tv both L2-normalized -> dot == cosine */
        for (int k = 0; k < HID; k++) d += (double)qv[k] * tv[k];
        c[m].id = (uint32_t)i; c[m].s = (float)d; m++;   /* id carries the input index */
    }
    if (ec_traced())
        fprintf(stderr, "[embed] recall: cosine-ranked %d facts by query relevance\n", m);
    qsort(c, (size_t)m, sizeof *c, rankcand_cmp);
    int r = m < max ? m : max;
    for (int i = 0; i < r; i++) out_order[i] = (int)c[i].id;
    free(c);
    return r;
}

#ifdef EMBED_TEST
/* Standalone validator: compares embed.c against tools/embed_prep/golden.txt
 * (token ids exact; vector cosine > 0.999). Build:
 *   gcc -O2 -std=c99 -DEMBED_TEST -o embed_test embed.c -lm */
static double cosine(const float *a, const float *b, int n) {
    double d = 0, na = 0, nb = 0;
    for (int i = 0; i < n; i++) { d += (double)a[i]*b[i]; na += (double)a[i]*a[i]; nb += (double)b[i]*b[i]; }
    return d / (sqrt(na) * sqrt(nb) + 1e-12);
}
int main(int argc, char **argv) {
    const char *dir = argc > 1 ? argv[1] : "models/bge-small-en-v1.5";
    const char *golden = argc > 2 ? argv[2] : "tools/embed_prep/golden.txt";
    char binp[512], vocp[512];
    snprintf(binp, sizeof binp, "%s/minilm.int8.bin", dir);
    snprintf(vocp, sizeof vocp, "%s/vocab.txt", dir);
    EmbedModel *m = embed_load(binp, vocp);
    if (!m) { fprintf(stderr, "load failed (%s, %s)\n", binp, vocp); return 1; }
    FILE *g = fopen(golden, "r");
    if (!g) { fprintf(stderr, "no golden %s\n", golden); return 1; }
    char *ln = NULL; size_t cap = 0; int fails = 0, cases = 0;
    while (getline(&ln, &cap, g) > 0) {
        char *t1 = strchr(ln, '\t'); if (!t1) continue; *t1++ = 0;
        char *t2 = strchr(t1, '\t'); if (!t2) continue; *t2++ = 0;
        const char *text = ln;
        /* parse golden ids */
        int gids[MAXPOS], gn = 0;
        for (char *p = strtok(t1, ","); p && gn < MAXPOS; p = strtok(NULL, ",")) gids[gn++] = atoi(p);
        /* parse golden vector */
        float gv[HID]; int gvn = 0;
        for (char *p = strtok(t2, ","); p && gvn < HID; p = strtok(NULL, ",")) gv[gvn++] = (float)atof(p);
        /* our tokenization + embedding */
        int ids[MAXPOS], n = embed_tokenize(m, text, ids, MAXPOS);
        int tok_ok = (n == gn);
        for (int i = 0; tok_ok && i < n; i++) tok_ok = (ids[i] == gids[i]);
        float ov[HID]; embed_text(m, text, ov);
        double cs = cosine(ov, gv, HID);
        int ok = tok_ok && cs > 0.999;
        printf("[%s] %-28s tokens %d/%d  cos %.6f\n", ok ? "PASS" : "FAIL", text, n, gn, cs);
        cases++; if (!ok) fails++;
    }
    free(ln); fclose(g); embed_free(m);
    printf("%d/%d passed\n", cases - fails, cases);
    return fails ? 1 : 0;
}
#endif
