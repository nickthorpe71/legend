/* legend.c — Legend v3, C build (Track 1 of v3_redesign.md §14).
 *
 * One file, C99 + POSIX-adjacent libc (flock, fsync, rename, clock_gettime).
 * Section order follows v3_c_plan.md §2; the S-number banners are
 * work-package boundaries — later WPs slot in between them.
 *
 * Status: M4 (dynamics + corpus) — S0–S10 complete: full packet + S7 live
 * dynamics (activation bump on touch, spaced-vs-massed stability, utility-
 * modulated decay with exception protection, importance effect-topology,
 * arousal salience bump). Links against libm (tanh in the stability cap).
 */

/* ========================================================================= *
 *  WHAT LEGEND IS
 *
 *  Legend is long-term memory for an LLM. The model calls this program to
 *  save facts it has learned and to recall an "orientation packet" — a
 *  compact, structured summary of what is currently true about a topic — so
 *  a fresh session can pick up where an old one left off. This one C file IS
 *  the whole tool: JSON comes in on stdin, JSON goes out on stdout, and a
 *  binary snapshot on disk is the only thing that persists between calls.
 *
 *  THE MENTAL MODEL (the vocabulary the whole file speaks; see v3_redesign.md
 *  §§5-8)
 *
 *  Everything stored is one of two things:
 *    - an ELEMENT: a named node. "coyote_time", "jump_height", "raycast",
 *      even the concept "uses" — all elements. An element has a canonical name,
 *      optional aliases, an optional one-line prose `summary`, and `stats`
 *      (numbers that track how used/important/trusted it is).
 *    - a RELATION: an edge with 2 to 5 named SLOTS, e.g.
 *      {subject: player_jump, uses: coyote_time}. Crucially the slot NAMES
 *      ("subject", "uses") are themselves elements. A relation can even point
 *      at another relation (that is how "this fact came from that source" is
 *      recorded). So the store is a hypergraph made of nothing but elements
 *      and slot-labeled edges between them.
 *
 *  Nothing is ever edited in place and nothing is deleted. "Current state" is
 *  maintained by SUPERSESSION: when jump_height changes from 3.5 to 4.2, the
 *  old value's relation is flipped to status Superseded and a new one is
 *  minted. History is the chain of superseded relations; the current answer
 *  is the one live relation at the end of it. Truth is append-only; the graph
 *  remembers how it got where it is.
 *
 *  A caller submits work with the `save` verb (a SUBMISSION: elements, facts,
 *  changes, retractions, merges...) and reads with `recall` (a FOCUS list, or
 *  nothing at all for a session-boot "orient me"). Both return a FRAME — the
 *  orientation packet — a JSON object of typed sections (what you focused on,
 *  the current state around it, recent activity, decisions, constraints, open
 *  questions, where the source material lives). A recall mints no new graph
 *  facts, but it is still a memory event: unless `observe:true`, retrieval
 *  advances the clock, reinforces the focused paths, runs decay, and persists
 *  those dynamics.
 *
 *  THE LIFE OF ONE INVOCATION (the spine every section serves)
 *
 *    read stdin  ->  lock the store  ->  load snapshot  ->
 *      save:   PLAN (validate/stage, touch NOTHING) -> APPLY (mutate)
 *      recall: resolve focus -> reinforce/decay unless observe:true
 *    ->  assemble frame  ->  save snapshot when mutated  ->  unlock  ->  print frame
 *
 *  The PLAN/APPLY split (S6, and v3_c_plan.md §4) is the save-path backbone.
 *  Plan does all the "can this even work?" checking against a read-only view
 *  of the graph; if any ref is bad it errors out and the store is left exactly
 *  as it was. Only once planning fully succeeds does apply run, and apply
 *  cannot fail (except on disk I/O). This is what makes a submission ALL-OR-NOTHING:
 *  a memory store that half-applied a retried write would double-count
 *  evidence and rot. Everything is built to make that guarantee structural.
 *
 *  C SURVIVAL KIT (idioms this file leans on; learn them once)
 *
 *  - One file. Like SQLite's amalgamation: no headers to keep in sync, no
 *    build system, `cc legend.c -lm -o legend`. The tests #include this file
 *    directly with LEGEND_NO_MAIN defined.
 *
 *  - u32 HANDLES, never pointers. Elements and relations live in flat, growable
 *    arrays called ARENAS. When an arena fills up it is realloc'd, and realloc
 *    is allowed to MOVE the whole block to a new address — so any raw pointer
 *    into it instantly dangles. The fix used everywhere: refer to things by
 *    their array index (a u32 "handle"), never by address. A pointer into an
 *    arena is only ever a short-lived local between two mutations. NONE_U32
 *    (0xFFFFFFFF) is the "null handle". This is iron rule 1 (plan §2).
 *
 *  - SPANS instead of copied strings. The 64 KiB stdin payload sits in one
 *    fixed buffer for the whole run. Rather than malloc/copy every little
 *    string out of it, code passes around a Span = {offset, length} that
 *    points INTO that buffer. Cheap, and it means strings aren't interned into
 *    the graph's string arena until apply actually decides to keep them.
 *
 *  - ONE error path: fail(). Any problem, at any call depth, calls
 *    fail(code, ...). fail() fills a global Err and then longjmp()s straight
 *    to the top of main. Think of longjmp as a `goto` that can jump OUT of any
 *    number of nested function calls at once (setjmp marks the landing spot).
 *    That is why no function returns error codes up a chain and no cleanup is
 *    threaded through: there is exactly one exit, and because planning mutates
 *    nothing, bailing out mid-plan leaks nothing important and corrupts
 *    nothing. (In test builds fail() longjmps back into the test instead of
 *    exiting, via g_err_trap.)
 *
 *  - DETERMINISM is a feature, not an accident. The same (store, payload,
 *    clock) must produce a BYTE-identical frame and snapshot — the conformance
 *    harness diffs two builds byte-for-byte, and replays must reproduce.
 *    Consequences you will see enforced: numbers are formatted by hand (never
 *    printf %f), output order is by id and never by hash-table iteration, and
 *    wall-clock time is injectable via the LEGEND_NOW env var so replays are
 *    reproducible.
 *
 *  READING ORDER (the S0-S10 banners, in the order they appear below)
 *
 *    S0  prelude       includes, limits, error codes, the fail() exit path
 *    S1  foundations   arenas, string interning, normalization, hashing,
 *                      trigram sets (the fuzzy-match primitive)
 *    S2  json          hand-rolled tokenizer + typed readers (stdin -> structs)
 *                      and the streaming writer + integer-only number formatter
 *    S3  model         Element/Relation/Attr/Term/Stats/Policy/Hypergraph and
 *                      the derived indices that make lookups fast
 *    S4  ontology      the built-in vocabulary + seed templates minted on init
 *    S5  resolution    turn a name/ref into an element id (tier 1 exact,
 *                      tier 2 trigram fuzzy), by position role
 *    S7  dynamics      the "physics": touch warms things up, neglect cools
 *                      availability, truth never decays  (defined before S6
 *                      because apply calls into it)
 *    S6  write path    PLAN then APPLY: the two-pass heart of the tool
 *    S8  frame         assemble the orientation packet and serialize it
 *    S9  persistence   snapshot byte-layout writer + validating reader, flock
 *    S10 cli           stdin read, verb dispatch, --pretty, main()
 * ========================================================================= */

/* ============================== S0 — prelude ==============================
 *
 * The bedrock every other section stands on: platform includes, the integer
 * typedefs (u32 etc.), the hard limits from the spec (payload/list caps), the
 * error-code enum, and — the important part — the single error exit, fail().
 *
 * fail() is worth understanding before anything else, because it is how EVERY
 * error in the program ends. It builds the JSON `{"error":{...}}` envelope
 * (spec §9) and either exits or, in tests, longjmps back to a trap. The two
 * small string helpers here (utf8_seq_len, json_escape_fputs) exist so that
 * error text — which can be arbitrary caller bytes — is always emitted as
 * valid, well-formed JSON no matter where it was truncated.
 */

#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <setjmp.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#include "embed.h" /* pure-C MiniLM: semantic miss-fallback (embed.c) */

typedef uint8_t u8;
typedef uint32_t u32;
typedef uint64_t u64;
typedef int32_t i32;
typedef int64_t i64;

#define NONE_U32 0xFFFFFFFFu

/* Build identity, stamped into the invocation journal and the init report so
 * a weeks-old store records which binary wrote each line. check.sh passes the
 * git short sha; a plain `cc legend.c embed.c` builds as "dev". */
#ifndef LEGEND_BUILD
#define LEGEND_BUILD "dev"
#endif

#if defined(__GNUC__) || defined(__clang__)
#define LEGEND_UNUSED __attribute__((unused))
#define LEGEND_NORETURN __attribute__((noreturn))
#else
#define LEGEND_UNUSED
#define LEGEND_NORETURN
#endif

enum {
  LEGEND_PAYLOAD_CAP = 64 * 1024, /* bytes; spec §5 */
  LEGEND_LIST_CAP = 64,           /* entries per payload list; spec §5 */
  LEGEND_LOCK_TIMEOUT_MS = 5000,  /* plan §3.12 */
  LEGEND_JSON_DEPTH_CAP = 128 /* nesting guard for the recursive tokenizer */
};

/* Intent scalars, indexed: conviction, prediction_error, arousal, curiosity
 * (spec §10). */
enum {
  INTENT_CONVICTION,
  INTENT_PREDICTION_ERROR,
  INTENT_AROUSAL,
  INTENT_CURIOSITY
};
static const double INTENT_DEFAULTS_SAVE[4] = {0.7, 0.3, 0.3, 0.2};
/* curiosity defaults to 0: novelty re-ranking of the recent band is opt-in
 * (a submitted curiosity > 0 lifts unfamiliar neighbors into the capped band). */
static const double INTENT_DEFAULTS_RECALL[4] = {0.3, 0.2, 0.3, 0.0};

/* Error codes (spec §9). */
enum {
  ERR_PARSE,
  ERR_UNKNOWN_REF,
  ERR_AMBIGUOUS_REF,
  ERR_LIMIT_EXCEEDED,
  ERR_NO_STORE,
  ERR_LOCK_TIMEOUT,
  ERR_SNAPSHOT_CORRUPT,
  ERR_STORE_FULL,
  ERR_PROSE_VALUE
};
static const char *const ERR_CODE_NAMES[] = {
    "parse",    "unknown_ref",  "ambiguous_ref",    "limit_exceeded",
    "no_store", "lock_timeout", "snapshot_corrupt", "store_full",
    "prose_value"};

/* A minted element name past this many (normalized) chars is prose, not a name.
 * Defined here — not in the audit section — so the save-time prose backstop and the
 * audit's prose_name check share ONE threshold. Mutable so tests can drive it. */
static u32 g_aud_name_chars = 120;  /* Measured on the trial store: median name
                                       29 bytes, p90 92, tail to 1177 — the tail is
                                       whole sentences passed where a canonical
                                       name belongs */

typedef struct {
  int code;
  char message[256];
  char at[192];          /* payload path, e.g. "facts[2].s"; empty = omitted */
  char candidates[2048]; /* prerendered JSON array text; empty = omitted */
} Err;

static Err g_err;
static jmp_buf g_err_jmp;
static int
    g_err_trap; /* tests set this; fail() then longjmps instead of exiting */

/* Length of the RFC 3629 UTF-8 sequence starting at s (shortest form only,
 * no surrogates, max U+10FFFF), bounded by the remaining bytes; 0 when the
 * bytes there are not a valid sequence. ASCII is a length-1 sequence. */
static u32 utf8_seq_len(const unsigned char *s, u32 remaining) {
  unsigned char c, lo = 0x80, hi = 0xBF;
  u32 n, k;
  if (remaining == 0)
    return 0;
  c = s[0];
  if (c < 0x80)
    return 1;
  if (c < 0xC2)
    return 0; /* continuation byte or overlong lead */
  if (c < 0xE0) {
    n = 2;
  } else if (c < 0xF0) {
    n = 3;
    if (c == 0xE0)
      lo = 0xA0; /* overlong */
    if (c == 0xED)
      hi = 0x9F; /* surrogate range */
  } else if (c < 0xF5) {
    n = 4;
    if (c == 0xF0)
      lo = 0x90; /* overlong */
    if (c == 0xF4)
      hi = 0x8F; /* beyond U+10FFFF */
  } else {
    return 0;
  }
  if (remaining < n)
    return 0;
  if (s[1] < lo || s[1] > hi)
    return 0;
  for (k = 2; k < n; k++)
    if (s[k] < 0x80 || s[k] > 0xBF)
      return 0;
  return n;
}

/* Minimal JSON string escaping for the error envelope and the init report
 * (the full streaming writer is WP1). Walks whole UTF-8 sequences and drops
 * malformed bytes — a fixed-size message buffer can cut a valid string
 * mid-sequence, and the envelope must stay valid JSON regardless. */
static void json_escape_fputs(const char *s, FILE *f) {
  u32 len = (u32)strlen(s), i = 0;
  while (i < len) {
    unsigned char c = (unsigned char)s[i];
    if (c == '"' || c == '\\') {
      fputc('\\', f);
      fputc(c, f);
      i++;
    } else if (c == '\n') {
      fputs("\\n", f);
      i++;
    } else if (c == '\r') {
      fputs("\\r", f);
      i++;
    } else if (c == '\t') {
      fputs("\\t", f);
      i++;
    } else if (c < 0x20) {
      fprintf(f, "\\u%04x", c);
      i++;
    } else if (c < 0x80) {
      fputc(c, f);
      i++;
    } else {
      u32 n = utf8_seq_len((const unsigned char *)s + i, len - i);
      if (n == 0) {
        i++;
        continue;
      } /* truncation artifact: drop */
      fwrite(s + i, 1, n, f);
      i += n;
    }
  }
}

/* The single error exit path: every failure funnels here (spec §9 envelope).
 * g_err_candidates holds prerendered candidate-array JSON staged by the
 * caller immediately before an ambiguous_ref/unknown_ref fail; fail()
 * consumes it, so an unstaged failure never leaks a previous list. */
static char g_err_candidates[2048];

static int g_pretty; /* --pretty: human rendering of frames and errors (S10) */

static void journal_fail(void); /* S10 sidecar journal: record the error line */

/* Raise an error and never return. `at` is an optional payload path
 * ("facts[2].s") pinpointing the offending input; the format args build the
 * human message. Emits the §9 error envelope as JSON (or a --pretty block),
 * then exits nonzero — unless a test has armed g_err_trap, in which case it
 * longjmps back so the test can inspect g_err. Because planning mutates
 * nothing, a longjmp out of mid-plan leaves the store untouched. */
LEGEND_NORETURN static void fail(int code, const char *at, const char *fmt,
                                 ...) {
  va_list ap;
  g_err.code = code;
  va_start(ap, fmt);
  vsnprintf(g_err.message, sizeof g_err.message, fmt, ap);
  va_end(ap);
  g_err.at[0] = 0;
  if (at)
    snprintf(g_err.at, sizeof g_err.at, "%s", at);
  memcpy(g_err.candidates, g_err_candidates, sizeof g_err.candidates);
  g_err_candidates[0] = 0;
  if (g_err_trap)
    longjmp(g_err_jmp, 1);
  journal_fail();
  if (g_pretty) {
    printf("error: %s\n  %s\n", ERR_CODE_NAMES[code], g_err.message);
    if (g_err.at[0])
      printf("  at: %s\n", g_err.at);
    if (g_err.candidates[0])
      printf("  candidates: %s\n", g_err.candidates);
    exit(1);
  }
  fputs("{\"error\":{\"code\":\"", stdout);
  fputs(ERR_CODE_NAMES[code], stdout);
  fputs("\",\"message\":\"", stdout);
  json_escape_fputs(g_err.message, stdout);
  fputc('"', stdout);
  if (g_err.at[0]) {
    fputs(",\"at\":\"", stdout);
    json_escape_fputs(g_err.at, stdout);
    fputc('"', stdout);
  }
  if (g_err.candidates[0]) {
    fputs(",\"candidates\":", stdout);
    fputs(g_err.candidates, stdout);
  }
  fputs("}}\n", stdout);
  exit(1);
}

/* Wall-clock "now" in unix seconds. Reads the real clock UNLESS LEGEND_NOW is
 * set — that env var is the determinism seam (spec §11): replays and golden
 * fixtures pin it so a frame is a pure function of (store, payload, clock). A
 * malformed override is a hard error, never a silent fall back to real time
 * (a typo must not quietly poison a fixture). */
LEGEND_UNUSED static i64 now_unix_seconds(void) {
  const char *env = getenv("LEGEND_NOW");
  if (env && *env) {
    char *end = NULL;
    long long v = strtoll(env, &end, 10);
    if (!end || *end != 0)
      fail(ERR_PARSE, NULL, "malformed LEGEND_NOW: %s", env);
    return (i64)v;
  }
  return (i64)time(NULL);
}

/* ============================ S1 — foundations ============================
 *
 * The data-structure toolkit the rest of the file is built from. Four ideas:
 *
 *  - ARENAS (xgrow, U32Vec): a growable array that doubles its capacity when
 *    full. realloc may move it, so callers hold u32 indices, not pointers.
 *
 *  - The STRING ARENA (StrArena) with INTERNING: every distinct string is
 *    stored once and given a small u32 id. "Interning" means: ask for a
 *    string, get back the id of the existing copy if present, otherwise add
 *    it. Then two strings are equal iff their ids are equal — a u32 compare
 *    instead of a memcmp. It uses OPEN ADDRESSING (a hash table with no linked
 *    lists: a string's hash picks a starting slot in an array, and if that
 *    slot is taken you walk forward to the next until you find it or an empty
 *    one). find() is kept strictly non-mutating so the plan phase can look
 *    things up without interning (iron rule 2).
 *
 *  - NORMALIZATION (normalize_name): the deterministic byte-level rule that
 *    makes "Jump Physics", "jump_physics" and "jump-physics" the SAME name
 *    (lowercase ASCII, collapse punctuation/space runs to one space, trim).
 *    This is tier-1 matching (spec §8) and must be copied verbatim by the
 *    Rust build and the differ, so it is hand-rolled with no ctype.h/locale.
 *
 *  - TRIGRAM SETS (trigram_set_build + jaccard/containment): the fuzzy-match
 *    primitive. Chop a string into every overlapping 3-byte window; two
 *    strings are "similar" when their window sets overlap a lot. Jaccard =
 *    overlap/union (symmetric); containment = overlap/|query| (lets a short
 *    phrase score high against a long summary it sits inside). Used by S5's
 *    tier-2 resolution and by near-match reporting.
 */

/* Grow-by-doubling backing store. Arenas realloc: cross-references are u32
 * handles, never pointers held across mutations (plan §2 iron rule 1). */
static void *xgrow(void *ptr, u32 needed, u32 *cap, size_t elem_size) {
  u32 ncap;
  if (needed <= *cap)
    return ptr;
  ncap = *cap ? *cap : 8;
  while (ncap < needed) {
    if (ncap > 0x80000000u) {
      fprintf(stderr, "legend: allocation too large\n");
      exit(1);
    }
    ncap *= 2;
  }
  ptr = realloc(ptr, (size_t)ncap * elem_size);
  if (!ptr) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  *cap = ncap;
  return ptr;
}

typedef struct {
  u32 *v;
  u32 count, cap;
} U32Vec;

static void u32vec_push(U32Vec *a, u32 x) {
  a->v = (u32 *)xgrow(a->v, a->count + 1, &a->cap, sizeof *a->v);
  a->v[a->count++] = x;
}

LEGEND_UNUSED static int u32vec_has(const U32Vec *v, u32 x) {
  u32 i;
  for (i = 0; i < v->count; i++)
    if (v->v[i] == x)
      return 1;
  return 0;
}

LEGEND_UNUSED static void u32vec_push_unique(U32Vec *v, u32 x) {
  if (!u32vec_has(v, x))
    u32vec_push(v, x);
}

/* qsort demands a non-null base even at count 0 (UBSan enforces it). */
static void u32vec_sort(U32Vec *v, int (*cmp)(const void *, const void *)) {
  if (v->count > 1)
    qsort(v->v, v->count, sizeof *v->v, cmp);
}

static u32 fnv1a(const char *bytes, u32 len) {
  u32 h = 2166136261u;
  u32 i;
  for (i = 0; i < len; i++) {
    h ^= (unsigned char)bytes[i];
    h *= 16777619u;
  }
  return h;
}

/* Valid UTF-8 end to end (RFC 3629). Both byte doors — payload strings and
 * snapshot strings — require it, so every string in the graph is valid UTF-8
 * by construction and frames are always well-formed JSON text. */
static int utf8_valid(const char *bytes, u32 len) {
  const unsigned char *s = (const unsigned char *)bytes;
  u32 i = 0;
  while (i < len) {
    u32 n;
    if (s[i] < 0x80) {
      i++;
      continue;
    }
    n = utf8_seq_len(s + i, len - i);
    if (n == 0)
      return 0;
    i += n;
  }
  return 1;
}

/* Name normalization, byte-level and locale-free (plan §3.2): fold A-Z via an
 * explicit range test, collapse separator runs to one space, trim the ends;
 * every other byte (incl. all non-ASCII UTF-8) passes through untouched.
 * No ctype.h, no setlocale() anywhere in this file. Returns the normalized
 * length; 0 means empty-after-normalize, a parse-class error the caller
 * raises. out must hold at least len bytes (normalization never grows). */
static int is_separator_byte(unsigned char c) {
  switch (c) {
  case ' ':
  case '\t':
  case '\n':
  case '\r':
  case '\v':
  case '\f':
  case '-':
  case '_':
  case '.':
  case ',':
  case ';':
  case ':':
  case '!':
  case '?':
  case '\'':
  case '"':
  case '(':
  case ')':
    return 1;
  default:
    return 0;
  }
}

LEGEND_UNUSED static u32 normalize_name(const char *s, u32 len, char *out) {
  u32 i, w = 0;
  int pending_space = 0;
  for (i = 0; i < len; i++) {
    unsigned char c = (unsigned char)s[i];
    if (is_separator_byte(c)) {
      if (w)
        pending_space = 1;
      continue;
    }
    if (c >= 'A' && c <= 'Z')
      c |= 0x20;
    if (pending_space) {
      out[w++] = ' ';
      pending_space = 0;
    }
    out[w++] = (char)c;
  }
  return w;
}

/* Interning string arena. find() never mutates — plan-phase lookups run on it
 * (plan §2 iron rule 2); intern() appends. Ids are dense u32 handles; bytes
 * are NUL-terminated in the backing store for printf convenience. */
typedef struct {
  char *bytes;
  u32 bytes_len, bytes_cap;
  u32 *offs;
  u32 count, cap;
  u32 *lens;
  u32 lens_cap;
  u32 *slots;
  u32 slot_cap, slot_used; /* open addressing; slot holds id+1; 0 = empty */
} StrArena;

static const char *str_ptr(const StrArena *a, u32 id) {
  return a->bytes + a->offs[id];
}
static u32 str_len(const StrArena *a, u32 id) { return a->lens[id]; }

static u32 str_find(const StrArena *a, const char *s, u32 len) {
  u32 mask, i;
  if (a->slot_cap == 0)
    return NONE_U32;
  mask = a->slot_cap - 1;
  i = fnv1a(s, len) & mask;
  while (a->slots[i]) {
    u32 id = a->slots[i] - 1;
    if (a->lens[id] == len && memcmp(a->bytes + a->offs[id], s, len) == 0)
      return id;
    i = (i + 1) & mask;
  }
  return NONE_U32;
}

static void str_arena_slot_insert(StrArena *a, u32 id) {
  u32 mask = a->slot_cap - 1;
  u32 i = fnv1a(a->bytes + a->offs[id], a->lens[id]) & mask;
  while (a->slots[i])
    i = (i + 1) & mask;
  a->slots[i] = id + 1;
  a->slot_used++;
}

static void str_arena_rehash(StrArena *a, u32 new_cap) {
  u32 id;
  free(a->slots);
  a->slots = (u32 *)calloc(new_cap, sizeof *a->slots);
  if (!a->slots) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  a->slot_cap = new_cap;
  a->slot_used = 0;
  for (id = 0; id < a->count; id++)
    str_arena_slot_insert(a, id);
}

LEGEND_UNUSED static u32 str_intern(StrArena *a, const char *s, u32 len) {
  u32 id = str_find(a, s, len);
  if (id != NONE_U32)
    return id;
  if (a->slot_cap == 0 || a->slot_used + 1 > a->slot_cap - a->slot_cap / 4)
    str_arena_rehash(a, a->slot_cap ? a->slot_cap * 2 : 16);
  a->bytes = (char *)xgrow(a->bytes, a->bytes_len + len + 1, &a->bytes_cap, 1);
  memcpy(a->bytes + a->bytes_len, s, len);
  a->bytes[a->bytes_len + len] = 0;
  a->offs = (u32 *)xgrow(a->offs, a->count + 1, &a->cap, sizeof *a->offs);
  a->lens = (u32 *)xgrow(a->lens, a->count + 1, &a->lens_cap, sizeof *a->lens);
  id = a->count;
  a->offs[id] = a->bytes_len;
  a->lens[id] = len;
  a->bytes_len += len + 1;
  a->count++;
  str_arena_slot_insert(a, id);
  return id;
}

LEGEND_UNUSED static void str_arena_free(StrArena *a) {
  free(a->bytes);
  free(a->offs);
  free(a->lens);
  free(a->slots);
  memset(a, 0, sizeof *a);
}

/* Open-addressing map from strings to u32 values. Keys are ids in a bound
 * StrArena (interned once, hashed by content); used over normalized names. */
typedef struct {
  u32 *keys; /* arena string id; NONE_U32 = empty slot */
  u32 *vals;
  u32 cap, used;
} StrMap;

LEGEND_UNUSED static u32 smap_get(const StrMap *m, const StrArena *a,
                                  const char *s, u32 len) {
  u32 mask, i;
  if (m->cap == 0)
    return NONE_U32;
  mask = m->cap - 1;
  i = fnv1a(s, len) & mask;
  while (m->keys[i] != NONE_U32) {
    u32 id = m->keys[i];
    if (str_len(a, id) == len && memcmp(str_ptr(a, id), s, len) == 0)
      return m->vals[i];
    i = (i + 1) & mask;
  }
  return NONE_U32;
}

static void smap_grow(StrMap *m, const StrArena *a, u32 new_cap) {
  u32 *old_keys = m->keys, *old_vals = m->vals;
  u32 old_cap = m->cap, i;
  m->keys = (u32 *)malloc((size_t)new_cap * sizeof *m->keys);
  m->vals = (u32 *)malloc((size_t)new_cap * sizeof *m->vals);
  if (!m->keys || !m->vals) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  for (i = 0; i < new_cap; i++)
    m->keys[i] = NONE_U32;
  m->cap = new_cap;
  m->used = 0;
  for (i = 0; i < old_cap; i++) {
    if (old_keys[i] == NONE_U32)
      continue;
    {
      u32 id = old_keys[i];
      u32 mask = new_cap - 1;
      u32 s = fnv1a(str_ptr(a, id), str_len(a, id)) & mask;
      while (m->keys[s] != NONE_U32)
        s = (s + 1) & mask;
      m->keys[s] = id;
      m->vals[s] = old_vals[i];
      m->used++;
    }
  }
  free(old_keys);
  free(old_vals);
}

/* key_id must be interned in the bound arena, so id equality is content
 * equality. */
LEGEND_UNUSED static void smap_put(StrMap *m, const StrArena *a, u32 key_id,
                                   u32 val) {
  u32 mask, i;
  if (m->cap == 0 || m->used + 1 > m->cap - m->cap / 4)
    smap_grow(m, a, m->cap ? m->cap * 2 : 16);
  mask = m->cap - 1;
  i = fnv1a(str_ptr(a, key_id), str_len(a, key_id)) & mask;
  while (m->keys[i] != NONE_U32) {
    if (m->keys[i] == key_id) {
      m->vals[i] = val;
      return;
    }
    i = (i + 1) & mask;
  }
  m->keys[i] = key_id;
  m->vals[i] = val;
  m->used++;
}

LEGEND_UNUSED static void smap_free(StrMap *m) {
  free(m->keys);
  free(m->vals);
  memset(m, 0, sizeof *m);
}

static int u32_cmp(const void *pa, const void *pb) {
  u32 x = *(const u32 *)pa, y = *(const u32 *)pb;
  return x < y ? -1 : x > y ? 1 : 0;
}

/* Trigram set over the bytes of a (normalized) string: every 3-byte window
 * packed big-endian into a u32. Inputs shorter than 3 bytes yield one
 * partial-window value so short names still compare. Output sorted, unique. */
LEGEND_UNUSED static void trigram_set_build(const char *s, u32 len,
                                            U32Vec *out) {
  u32 i;
  out->count = 0;
  if (len == 0)
    return;
  if (len < 3) {
    u32 v = (u32)(unsigned char)s[0] << 16;
    if (len == 2)
      v |= (u32)(unsigned char)s[1] << 8;
    u32vec_push(out, v);
    return;
  }
  for (i = 0; i + 3 <= len; i++) {
    u32 v = (u32)(unsigned char)s[i] << 16 | (u32)(unsigned char)s[i + 1] << 8 |
            (u32)(unsigned char)s[i + 2];
    u32vec_push(out, v);
  }
  u32vec_sort(out, u32_cmp);
  {
    u32 w = 0;
    for (i = 0; i < out->count; i++)
      if (w == 0 || out->v[w - 1] != out->v[i])
        out->v[w++] = out->v[i];
    out->count = w;
  }
}

static u32 trigram_intersect(const u32 *a, u32 an, const u32 *b, u32 bn) {
  u32 i = 0, j = 0, inter = 0;
  while (i < an && j < bn) {
    if (a[i] == b[j]) {
      inter++;
      i++;
      j++;
    } else if (a[i] < b[j])
      i++;
    else
      j++;
  }
  return inter;
}

/* Jaccard over two u32-sorted unique trigram arrays; 0.0 when both are empty.
 */
LEGEND_UNUSED static double trigram_jaccard(const u32 *a, u32 an, const u32 *b,
                                            u32 bn) {
  u32 inter = trigram_intersect(a, an, b, bn);
  u32 uni = an + bn - inter;
  return uni ? (double)inter / (double)uni : 0.0;
}

/* Containment of q in t: |q ∩ t| / |q|. The read-position tier-2 measure —
 * a short focus phrase buried in a long summary still scores high, which
 * symmetric Jaccard structurally cannot do ("jump feel" vs a 70-byte
 * summary shares 7 trigrams of a ~70-trigram union). */
LEGEND_UNUSED static double trigram_containment(const u32 *q, u32 qn,
                                                const u32 *t, u32 tn) {
  return qn ? (double)trigram_intersect(q, qn, t, tn) / (double)qn : 0.0;
}

/* =============================== S2 — json ===============================
 *
 * All the JSON, in three layers, kept deliberately minimal (a small parser is
 * an auditable parser — see the risk table in plan §10):
 *
 *  1. TOKENIZER (jparse_*, json_parse): turns the payload text into a flat
 *     array of tokens — no tree, no allocations per node. Each token is just
 *     a type plus a byte range into the payload buffer. This is a two-part
 *     design on purpose: the tokenizer knows JSON grammar but nothing about
 *     Legend's schema.
 *
 *  2. TYPED READERS (rd_*, read_element/read_fact/read_submission/...): walk
 *     the token array and pull out exactly the fields Legend expects, checking
 *     shapes and raising `parse` errors with a payload path (`at:"facts[2].s"`)
 *     on anything unexpected — including UNKNOWN fields (pin 1: a silently
 *     ignored typo is a lost write). Readers produce Spans (offset+length into
 *     the payload), never interned strings — interning is apply's job, so a
 *     rejected submission mutates nothing (iron rule 2).
 *
 *  3. STREAMING WRITER (further down: json_escape_fwrite and the frame
 *     printers in S8): emits the frame straight to stdout, no DOM built up.
 *     The number formatter here is special: it prints the handful of [0,1]
 *     floats (score, confidence) with pure integer arithmetic (pin 7) so C and
 *     Rust produce byte-identical text — libc's %f is never called.
 *
 * WHY tokenize instead of build a tree: strings are unescaped IN PLACE inside
 * the payload buffer (the unescaped form is never longer than the escaped
 * form, so it always fits), and tokens just point at ranges. Zero heap churn,
 * and it dovetails with the "don't intern until apply" rule.
 */

/* Tokenizer, jsmn-style: a flat token array over the payload buffer, strings
 * unescaped in place (unescaped text is never longer than its escaped form).
 * Tokens carry (start,end) byte offsets into the payload buffer — nothing is
 * interned at parse time (plan §2 iron rule 2). */

enum { J_OBJ, J_ARR, J_STR, J_NUM, J_TRUE, J_FALSE, J_NULL };

typedef struct {
  u8 type;
  u32 start, end; /* byte range in the payload buffer; for J_STR, the unescaped
                     bytes */
  u32 size;       /* J_OBJ: pair count; J_ARR: element count */
} Tok;

typedef struct {
  char *buf;
  u32 len, pos, depth;
  Tok *toks;
  u32 count, cap;
} Json;

LEGEND_UNUSED static void json_free(Json *j) {
  free(j->toks);
  memset(j, 0, sizeof *j);
}

static u32 jtok_push(Json *j, u8 type, u32 start) {
  j->toks = (Tok *)xgrow(j->toks, j->count + 1, &j->cap, sizeof *j->toks);
  j->toks[j->count].type = type;
  j->toks[j->count].start = start;
  j->toks[j->count].end = start;
  j->toks[j->count].size = 0;
  return j->count++;
}

static void jskip_ws(Json *j) {
  while (j->pos < j->len) {
    char c = j->buf[j->pos];
    if (c == ' ' || c == '\t' || c == '\n' || c == '\r')
      j->pos++;
    else
      break;
  }
}

static int jhex_digit(char c) {
  if (c >= '0' && c <= '9')
    return c - '0';
  if (c >= 'a' && c <= 'f')
    return c - 'a' + 10;
  if (c >= 'A' && c <= 'F')
    return c - 'A' + 10;
  return -1;
}

static u32 jparse_hex4(Json *j, u32 *rp) {
  u32 v = 0;
  int k;
  if (*rp + 4 > j->len)
    fail(ERR_PARSE, NULL, "invalid JSON: truncated \\u escape at byte %u", *rp);
  for (k = 0; k < 4; k++) {
    int d = jhex_digit(j->buf[*rp + (u32)k]);
    if (d < 0)
      fail(ERR_PARSE, NULL, "invalid JSON: bad \\u escape at byte %u", *rp);
    v = v * 16 + (u32)d;
  }
  *rp += 4;
  return v;
}

/* Scan a "..." string, unescaping IN PLACE: rp reads ahead, wp writes the
 * decoded bytes behind it (safe because a decode is never longer than its
 * source). Rejects unescaped controls and invalid UTF-8. A \uXXXX escape is
 * decoded to UTF-8; a high surrogate (D800-DBFF) must be immediately followed
 * by a low surrogate (DC00-DFFF), and the pair is combined into one code point
 * — that is how JSON spells characters above U+FFFF. The token's byte range is
 * then the unescaped span [start, wp). */
static void jparse_string(Json *j) {
  u32 ti = jtok_push(j, J_STR, j->pos + 1);
  u32 rp = j->pos + 1, wp = j->pos + 1;
  for (;;) {
    unsigned char c;
    if (rp >= j->len)
      fail(ERR_PARSE, NULL, "invalid JSON: unterminated string at byte %u",
           j->toks[ti].start - 1);
    c = (unsigned char)j->buf[rp];
    if (c == '"') {
      j->toks[ti].end = wp;
      if (!utf8_valid(j->buf + j->toks[ti].start, wp - j->toks[ti].start))
        fail(ERR_PARSE, NULL,
             "invalid JSON: string is not valid UTF-8 at byte %u",
             j->toks[ti].start - 1);
      j->pos = rp + 1;
      return;
    }
    if (c < 0x20)
      fail(ERR_PARSE, NULL,
           "invalid JSON: unescaped control character in string at byte %u",
           rp);
    if (c != '\\') {
      j->buf[wp++] = (char)c;
      rp++;
      continue;
    }
    rp++;
    if (rp >= j->len)
      fail(ERR_PARSE, NULL, "invalid JSON: truncated escape at byte %u",
           rp - 1);
    switch (j->buf[rp++]) {
    case '"':
      j->buf[wp++] = '"';
      break;
    case '\\':
      j->buf[wp++] = '\\';
      break;
    case '/':
      j->buf[wp++] = '/';
      break;
    case 'b':
      j->buf[wp++] = '\b';
      break;
    case 'f':
      j->buf[wp++] = '\f';
      break;
    case 'n':
      j->buf[wp++] = '\n';
      break;
    case 'r':
      j->buf[wp++] = '\r';
      break;
    case 't':
      j->buf[wp++] = '\t';
      break;
    case 'u': {
      u32 cp = jparse_hex4(j, &rp);
      if (cp >= 0xDC00 && cp <= 0xDFFF)
        fail(ERR_PARSE, NULL, "invalid JSON: lone low surrogate at byte %u",
             rp - 6);
      if (cp >= 0xD800 && cp <= 0xDBFF) {
        u32 lo;
        if (rp + 2 > j->len || j->buf[rp] != '\\' || j->buf[rp + 1] != 'u')
          fail(ERR_PARSE, NULL, "invalid JSON: lone high surrogate at byte %u",
               rp - 6);
        rp += 2;
        lo = jparse_hex4(j, &rp);
        if (lo < 0xDC00 || lo > 0xDFFF)
          fail(ERR_PARSE, NULL, "invalid JSON: bad surrogate pair at byte %u",
               rp - 6);
        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
      }
      if (cp < 0x80) {
        j->buf[wp++] = (char)cp;
      } else if (cp < 0x800) {
        j->buf[wp++] = (char)(0xC0 | (cp >> 6));
        j->buf[wp++] = (char)(0x80 | (cp & 0x3F));
      } else if (cp < 0x10000) {
        j->buf[wp++] = (char)(0xE0 | (cp >> 12));
        j->buf[wp++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        j->buf[wp++] = (char)(0x80 | (cp & 0x3F));
      } else {
        j->buf[wp++] = (char)(0xF0 | (cp >> 18));
        j->buf[wp++] = (char)(0x80 | ((cp >> 12) & 0x3F));
        j->buf[wp++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        j->buf[wp++] = (char)(0x80 | (cp & 0x3F));
      }
      break;
    }
    default:
      fail(ERR_PARSE, NULL, "invalid JSON: bad escape at byte %u", rp - 1);
    }
  }
}

static void jparse_number(Json *j) {
  u32 ti = jtok_push(j, J_NUM, j->pos);
  if (j->pos < j->len && j->buf[j->pos] == '-')
    j->pos++;
  if (j->pos >= j->len || j->buf[j->pos] < '0' || j->buf[j->pos] > '9')
    fail(ERR_PARSE, NULL, "invalid JSON: bad number at byte %u",
         j->toks[ti].start);
  if (j->buf[j->pos] == '0') {
    j->pos++;
    if (j->pos < j->len && j->buf[j->pos] >= '0' && j->buf[j->pos] <= '9')
      fail(ERR_PARSE, NULL, "invalid JSON: leading zero at byte %u",
           j->toks[ti].start);
  } else {
    while (j->pos < j->len && j->buf[j->pos] >= '0' && j->buf[j->pos] <= '9')
      j->pos++;
  }
  if (j->pos < j->len && j->buf[j->pos] == '.') {
    j->pos++;
    if (j->pos >= j->len || j->buf[j->pos] < '0' || j->buf[j->pos] > '9')
      fail(ERR_PARSE, NULL, "invalid JSON: bad number at byte %u",
           j->toks[ti].start);
    while (j->pos < j->len && j->buf[j->pos] >= '0' && j->buf[j->pos] <= '9')
      j->pos++;
  }
  if (j->pos < j->len && (j->buf[j->pos] == 'e' || j->buf[j->pos] == 'E')) {
    j->pos++;
    if (j->pos < j->len && (j->buf[j->pos] == '+' || j->buf[j->pos] == '-'))
      j->pos++;
    if (j->pos >= j->len || j->buf[j->pos] < '0' || j->buf[j->pos] > '9')
      fail(ERR_PARSE, NULL, "invalid JSON: bad number at byte %u",
           j->toks[ti].start);
    while (j->pos < j->len && j->buf[j->pos] >= '0' && j->buf[j->pos] <= '9')
      j->pos++;
  }
  j->toks[ti].end = j->pos;
}

static void jparse_value(Json *j);

static void jparse_literal(Json *j, const char *word, u32 word_len, u8 type) {
  if (j->pos + word_len > j->len ||
      memcmp(j->buf + j->pos, word, word_len) != 0)
    fail(ERR_PARSE, NULL, "invalid JSON: unexpected character at byte %u",
         j->pos);
  jtok_push(j, type, j->pos);
  j->toks[j->count - 1].end = j->pos + word_len;
  j->pos += word_len;
}

static void jparse_object(Json *j) {
  u32 oi = jtok_push(j, J_OBJ, j->pos);
  if (++j->depth > LEGEND_JSON_DEPTH_CAP)
    fail(ERR_PARSE, NULL, "invalid JSON: nesting deeper than %d",
         LEGEND_JSON_DEPTH_CAP);
  j->pos++;
  jskip_ws(j);
  if (j->pos < j->len && j->buf[j->pos] == '}') {
    j->pos++;
    j->toks[oi].end = j->pos;
    j->depth--;
    return;
  }
  for (;;) {
    jskip_ws(j);
    if (j->pos >= j->len || j->buf[j->pos] != '"')
      fail(ERR_PARSE, NULL, "invalid JSON: expected object key at byte %u",
           j->pos);
    jparse_string(j);
    jskip_ws(j);
    if (j->pos >= j->len || j->buf[j->pos] != ':')
      fail(ERR_PARSE, NULL, "invalid JSON: expected ':' at byte %u", j->pos);
    j->pos++;
    jparse_value(j);
    j->toks[oi].size++;
    jskip_ws(j);
    if (j->pos >= j->len)
      fail(ERR_PARSE, NULL, "invalid JSON: unterminated object at byte %u",
           j->toks[oi].start);
    if (j->buf[j->pos] == ',') {
      j->pos++;
      continue;
    }
    if (j->buf[j->pos] == '}') {
      j->pos++;
      j->toks[oi].end = j->pos;
      j->depth--;
      return;
    }
    fail(ERR_PARSE, NULL, "invalid JSON: expected ',' or '}' at byte %u",
         j->pos);
  }
}

static void jparse_array(Json *j) {
  u32 ai = jtok_push(j, J_ARR, j->pos);
  if (++j->depth > LEGEND_JSON_DEPTH_CAP)
    fail(ERR_PARSE, NULL, "invalid JSON: nesting deeper than %d",
         LEGEND_JSON_DEPTH_CAP);
  j->pos++;
  jskip_ws(j);
  if (j->pos < j->len && j->buf[j->pos] == ']') {
    j->pos++;
    j->toks[ai].end = j->pos;
    j->depth--;
    return;
  }
  for (;;) {
    jparse_value(j);
    j->toks[ai].size++;
    jskip_ws(j);
    if (j->pos >= j->len)
      fail(ERR_PARSE, NULL, "invalid JSON: unterminated array at byte %u",
           j->toks[ai].start);
    if (j->buf[j->pos] == ',') {
      j->pos++;
      continue;
    }
    if (j->buf[j->pos] == ']') {
      j->pos++;
      j->toks[ai].end = j->pos;
      j->depth--;
      return;
    }
    fail(ERR_PARSE, NULL, "invalid JSON: expected ',' or ']' at byte %u",
         j->pos);
  }
}

static void jparse_value(Json *j) {
  char c;
  jskip_ws(j);
  if (j->pos >= j->len)
    fail(ERR_PARSE, NULL, "invalid JSON: unexpected end of input");
  c = j->buf[j->pos];
  if (c == '{')
    jparse_object(j);
  else if (c == '[')
    jparse_array(j);
  else if (c == '"')
    jparse_string(j);
  else if (c == '-' || (c >= '0' && c <= '9'))
    jparse_number(j);
  else if (c == 't')
    jparse_literal(j, "true", 4, J_TRUE);
  else if (c == 'f')
    jparse_literal(j, "false", 5, J_FALSE);
  else if (c == 'n')
    jparse_literal(j, "null", 4, J_NULL);
  else
    fail(ERR_PARSE, NULL, "invalid JSON: unexpected character at byte %u",
         j->pos);
}

/* Tokenize buf[0..len). Reuses j's token storage across calls; fails via the
 * single error path on any syntax error, truncation, or trailing content. */
static void json_parse(Json *j, char *buf, u32 len) {
  j->buf = buf;
  j->len = len;
  j->pos = 0;
  j->depth = 0;
  j->count = 0;
  jparse_value(j);
  jskip_ws(j);
  if (j->pos != j->len)
    fail(ERR_PARSE, NULL, "invalid JSON: trailing content at byte %u", j->pos);
}

/* Index of the token just past subtree i (tokens are stored in document order).
 */
static u32 tok_skip(const Tok *t, u32 i) {
  u32 next = i + 1, k, n = t[i].size;
  if (t[i].type == J_OBJ) {
    for (k = 0; k < n; k++)
      next = tok_skip(t, next + 1); /* key, then value subtree */
  } else if (t[i].type == J_ARR) {
    for (k = 0; k < n; k++)
      next = tok_skip(t, next);
  }
  return next;
}

/* --- S2 typed schema readers ---------------------------------------------
 * Everything below validates shape and records (offset,len) spans into the
 * payload buffer; nothing is interned or resolved here — that is plan/apply
 * work (S5/S6, plan §4). Errors carry a payload path in `at`, spec §9 style
 * ("facts[2].s"). */

typedef struct {
  u32 off, len;
} Span;

static Span span_none(void) {
  Span s;
  s.off = NONE_U32;
  s.len = 0;
  return s;
}
static int span_absent(Span s) { return s.off == NONE_U32; }

typedef struct {
  const Tok *t;
  const char *buf;
} Rd;

static u32 rd_len(const Rd *r, u32 i) { return r->t[i].end - r->t[i].start; }
static const char *rd_ptr(const Rd *r, u32 i) { return r->buf + r->t[i].start; }
static Span rd_span(const Rd *r, u32 i) {
  Span s;
  s.off = r->t[i].start;
  s.len = r->t[i].end - r->t[i].start;
  return s;
}

static int rd_str_eq(const Rd *r, u32 i, const char *lit) {
  u32 n = rd_len(r, i);
  return strlen(lit) == n && memcmp(rd_ptr(r, i), lit, n) == 0;
}

LEGEND_NORETURN static void fail_unknown_field(const Rd *r, u32 key_tok,
                                               const char *parent_path) {
  char at[192];
  u32 n = rd_len(r, key_tok);
  if (n > 80)
    n = 80;
  if (parent_path && parent_path[0])
    snprintf(at, sizeof at, "%s.%.*s", parent_path, (int)n, rd_ptr(r, key_tok));
  else
    snprintf(at, sizeof at, "%.*s", (int)n, rd_ptr(r, key_tok));
  fail(ERR_PARSE, at, "unknown field \"%.*s\"", (int)n, rd_ptr(r, key_tok));
}

static Span rd_string(const Rd *r, u32 i, const char *path) {
  if (r->t[i].type != J_STR)
    fail(ERR_PARSE, path, "expected a string");
  return rd_span(r, i);
}

static Span rd_string_nonempty(const Rd *r, u32 i, const char *path) {
  Span s = rd_string(r, i, path);
  if (s.len == 0)
    fail(ERR_PARSE, path, "expected a non-empty string");
  return s;
}

static double rd_number(const Rd *r, u32 i, const char *path) {
  char tmp[64];
  u32 n = rd_len(r, i);
  if (r->t[i].type != J_NUM)
    fail(ERR_PARSE, path, "expected a number");
  if (n >= sizeof tmp)
    fail(ERR_PARSE, path, "number too long");
  memcpy(tmp, rd_ptr(r, i), n);
  tmp[n] = 0;
  return strtod(tmp, NULL);
}

static double rd_unit_float(const Rd *r, u32 i, const char *path) {
  double v = rd_number(r, i, path);
  if (!(v >= 0.0 && v <= 1.0))
    fail(ERR_PARSE, path, "expected a number in [0,1]");
  return v;
}

static int rd_bool(const Rd *r, u32 i, const char *path) {
  if (r->t[i].type == J_TRUE)
    return 1;
  if (r->t[i].type == J_FALSE)
    return 0;
  fail(ERR_PARSE, path, "expected true or false");
  return 0; /* unreachable */
}

static i64 rd_integer(const Rd *r, u32 i, const char *path) {
  char tmp[32];
  u32 k, n = rd_len(r, i);
  const char *p = rd_ptr(r, i);
  if (r->t[i].type != J_NUM)
    fail(ERR_PARSE, path, "expected an integer");
  for (k = 0; k < n; k++)
    if (p[k] == '.' || p[k] == 'e' || p[k] == 'E')
      fail(ERR_PARSE, path, "expected an integer");
  if (n >= sizeof tmp)
    fail(ERR_PARSE, path, "number too long");
  memcpy(tmp, p, n);
  tmp[n] = 0;
  return (i64)strtoll(tmp, NULL, 10);
}

/* Array type + the spec §5 per-list entry cap. Returns the element count. */
static u32 rd_array(const Rd *r, u32 i, const char *path) {
  if (r->t[i].type != J_ARR)
    fail(ERR_PARSE, path, "expected an array");
  if (r->t[i].size > LEGEND_LIST_CAP)
    fail(ERR_LIMIT_EXCEEDED, path, "list exceeds %d entries", LEGEND_LIST_CAP);
  return r->t[i].size;
}

/* Exactly "rel:<decimal>" (plan §3.9: no whitespace tolerance). */
static int span_is_rel_ref(const char *buf, Span s) {
  u32 k;
  if (s.len < 5 || memcmp(buf + s.off, "rel:", 4) != 0)
    return 0;
  for (k = 4; k < s.len; k++)
    if (buf[s.off + k] < '0' || buf[s.off + k] > '9')
      return 0;
  return 1;
}

/* --- parsed payload shapes ------------------------------------------------
 * The readers above fill these. They mirror the §5 submission JSON one-to-one,
 * but hold Spans (offset+length into the payload buffer) instead of copied
 * strings — nothing here is interned or resolved yet. The variable-length bits
 * (alias lists, attr maps, slot values) don't each own a malloc; instead they
 * are ranges into two shared arenas on the Submission — the "span pool" for
 * strings and the "attr pool" for attribute entries — referenced as
 * (start,count). span_none()/span_absent() mark an optional field that the
 * payload omitted. */

typedef struct {
  Span key;
  u32 val_start, val_count;
} AttrEntry; /* one attribute: its name span + a (start,count) slice of value
                spans in the span pool */

typedef struct {
  Span name, kind, summary;
  Span rename_to; /* pin 23: new canonical name */
  Span src; /* pin 26: expands to a {subject, src: pointer} base relation */
  u32 alias_start, alias_count; /* span pool */
  u32 attr_start, attr_count;   /* attr pool */
  double salience;
  int has_salience;
  double confidence;
  int has_confidence;
  int is_new;
} SubElement;

typedef struct {
  Span kind, summary;
  u32 expects_start, expects_count; /* span pool */
} SubTemplate;

enum { ST_ASSERTED, ST_ENTAILED, ST_DEFEASIBLE };

/* Behavioral-modal bit flags parsed from a fact's `modal` array and reified as
 * meta-relations on the fact (new_foundation §16.3): intervened marks Pearl
 * rung-2 agent action (vs default rung-1 observation); non_actual a claim not
 * about actual world state (counterfactual/desired); negated a polarity flip;
 * uncertain a graded hedge; general a habitual/generic claim. */
enum {
  MODF_INTERVENED = 1u << 0,
  MODF_NEGATED = 1u << 1,
  MODF_UNCERTAIN = 1u << 2,
  MODF_NON_ACTUAL = 1u << 3,
  MODF_GENERAL = 1u << 4
};

typedef struct {
  int is_triple;
  Span s, p, o;               /* triple form */
  u32 attr_start, attr_count; /* general form; attr pool */
  u8 status;
  double confidence;
  int has_confidence;
  double salience;
  int has_salience;
  Span src;
  u32 modal; /* MODF_* bitset, reified as meta-relations on the fact */
} SubFact;

typedef struct {
  Span target, property, from, to, event, src;
  int intervened;
  double confidence;
  int has_confidence;
} SubChange;

typedef struct {
  int is_rel_ref;
  Span rel_ref; /* the full "rel:<id>" span */
  SubFact fact; /* fact-shape form (no per-fact options) */
} SubRetract;

typedef struct {
  Span from, into;
} SubMerge;

/* The whole parsed `save` payload. One of these per invocation; the write
 * lists drive the plan phase in payload-walk order. A `recall` reuses only the
 * focus/intent/observe parts (it is a submission with no writes, spec §4). */
typedef struct {
  Span source; /* provenance string; attached to every relation minted this tick
                */
  SubElement *elements;
  u32 element_count, element_cap; /* explicit element declarations */
  SubTemplate *templates;
  u32 template_count,
      template_cap; /* on-the-fly memory-object type definitions */
  SubFact *facts;
  u32 fact_count, fact_cap; /* relations to assert */
  SubChange *changes;
  u32 change_count, change_cap; /* supersession writes (current-state flips) */
  SubRetract *retracts;
  u32 retract_count,
      retract_cap; /* corrections: flip a relation to Retracted */
  SubMerge *merges;
  u32 merge_count, merge_cap; /* caller-confirmed element folds */
  Span *focus;
  u32 focus_count,
      focus_cap;    /* optional read anchors; absent => compact frame */
  double intent[4]; /* verb defaults, overridden per key */
  int observe; /* rejected on save (pin 28): a write is not an observation */
  /* pools: variable-length fields above index into these instead of owning
   * mallocs */
  Span *span_pool;
  u32 span_pool_count, span_pool_cap;
  AttrEntry *attr_pool;
  u32 attr_pool_count, attr_pool_cap;
} Submission;

typedef struct {
  Span *focus;
  u32 focus_count, focus_cap;
  Span query;        /* optional F1 ranking query; .len==0 => fall back to focus */
  i64 limit;         /* > 0; -1 = null (uncapped); 40 when omitted */
  i32 history_depth; /* >= 0; -1 = null (full chains); 2 when omitted */
  i64 since;         /* unix seconds at UTC midnight; -1 = absent */
  double intent[4];  /* a recall is a submission with no writes (spec §4) */
  int observe;
} Recall;

LEGEND_UNUSED static void sub_free(Submission *s) {
  free(s->elements);
  free(s->templates);
  free(s->facts);
  free(s->changes);
  free(s->retracts);
  free(s->merges);
  free(s->focus);
  free(s->span_pool);
  free(s->attr_pool);
  memset(s, 0, sizeof *s);
}

LEGEND_UNUSED static void recall_free(Recall *r) {
  free(r->focus);
  memset(r, 0, sizeof *r);
}

static u32 sub_push_span(Submission *s, Span sp) {
  s->span_pool = (Span *)xgrow(s->span_pool, s->span_pool_count + 1,
                               &s->span_pool_cap, sizeof *s->span_pool);
  s->span_pool[s->span_pool_count] = sp;
  return s->span_pool_count++;
}

static void sub_push_attr(Submission *s, AttrEntry e) {
  s->attr_pool = (AttrEntry *)xgrow(s->attr_pool, s->attr_pool_count + 1,
                                    &s->attr_pool_cap, sizeof *s->attr_pool);
  s->attr_pool[s->attr_pool_count++] = e;
}

/* Array of non-empty strings into the span pool; returns (start,count) via out.
 */
static void read_string_list(const Rd *r, u32 arr_i, const char *path,
                             Submission *sub, u32 *out_start, u32 *out_count) {
  u32 n = rd_array(r, arr_i, path);
  u32 ti = arr_i + 1, k;
  *out_start = sub->span_pool_count;
  for (k = 0; k < n; k++) {
    char ipath[192];
    snprintf(ipath, sizeof ipath, "%s[%u]", path, k);
    sub_push_span(sub, rd_string_nonempty(r, ti, ipath));
    ti = tok_skip(r->t, ti);
  }
  *out_count = n;
}

/* attrs object: attribute-name -> ref, or array of refs (one relation per
 * value — spec §5 shape rule). */
static void read_attrs(const Rd *r, u32 obj_i, const char *path,
                       Submission *sub, u32 *out_start, u32 *out_count) {
  u32 n, ti, k;
  if (r->t[obj_i].type != J_OBJ)
    fail(ERR_PARSE, path, "expected an object");
  n = r->t[obj_i].size;
  if (n > LEGEND_LIST_CAP)
    fail(ERR_LIMIT_EXCEEDED, path, "attrs exceeds %d entries", LEGEND_LIST_CAP);
  *out_start = sub->attr_pool_count;
  ti = obj_i + 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    char vpath[192];
    AttrEntry e;
    u32 klen = rd_len(r, key);
    if (klen == 0)
      fail(ERR_PARSE, path, "empty attribute name");
    if (klen > 64)
      klen = 64;
    snprintf(vpath, sizeof vpath, "%s.%.*s", path, (int)klen, rd_ptr(r, key));
    e.key = rd_span(r, key);
    if (r->t[val].type == J_STR) {
      Span v = rd_string_nonempty(r, val, vpath);
      e.val_start = sub->span_pool_count;
      e.val_count = 1;
      sub_push_span(sub, v);
    } else if (r->t[val].type == J_ARR) {
      u32 c = rd_array(r, val, vpath);
      u32 vi = val + 1, m;
      if (c == 0)
        fail(ERR_PARSE, vpath, "expected at least one value");
      e.val_start = sub->span_pool_count;
      e.val_count = c;
      for (m = 0; m < c; m++) {
        char ipath[256];
        snprintf(ipath, sizeof ipath, "%s[%u]", vpath, m);
        sub_push_span(sub, rd_string_nonempty(r, vi, ipath));
        vi = tok_skip(r->t, vi);
      }
    } else {
      fail(ERR_PARSE, vpath, "expected a string or an array of strings");
    }
    sub_push_attr(sub, e);
    ti = tok_skip(r->t, val);
  }
  *out_count = n;
}

static void read_element(const Rd *r, u32 obj_i, const char *path,
                         Submission *sub, SubElement *e) {
  u32 n, ti, k;
  memset(e, 0, sizeof *e);
  e->name = span_none();
  e->kind = span_none();
  e->summary = span_none();
  e->rename_to = span_none();
  e->src = span_none();
  if (r->t[obj_i].type != J_OBJ)
    fail(ERR_PARSE, path, "expected an object");
  n = r->t[obj_i].size;
  ti = obj_i + 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    char vpath[192];
    if (rd_str_eq(r, key, "name")) {
      snprintf(vpath, sizeof vpath, "%s.name", path);
      e->name = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "kind")) {
      snprintf(vpath, sizeof vpath, "%s.kind", path);
      e->kind = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "aliases")) {
      snprintf(vpath, sizeof vpath, "%s.aliases", path);
      read_string_list(r, val, vpath, sub, &e->alias_start, &e->alias_count);
    } else if (rd_str_eq(r, key, "summary")) {
      snprintf(vpath, sizeof vpath, "%s.summary", path);
      e->summary = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "salience")) {
      snprintf(vpath, sizeof vpath, "%s.salience", path);
      e->salience = rd_unit_float(r, val, vpath);
      e->has_salience = 1;
    } else if (rd_str_eq(r, key, "confidence")) {
      snprintf(vpath, sizeof vpath, "%s.confidence", path);
      e->confidence = rd_unit_float(r, val, vpath);
      e->has_confidence = 1;
    } else if (rd_str_eq(r, key, "attrs")) {
      snprintf(vpath, sizeof vpath, "%s.attrs", path);
      read_attrs(r, val, vpath, sub, &e->attr_start, &e->attr_count);
    } else if (rd_str_eq(r, key, "new")) {
      snprintf(vpath, sizeof vpath, "%s.new", path);
      e->is_new = rd_bool(r, val, vpath);
    } else if (rd_str_eq(r, key, "rename_to")) {
      snprintf(vpath, sizeof vpath, "%s.rename_to", path);
      e->rename_to = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "src")) {
      snprintf(vpath, sizeof vpath, "%s.src", path);
      e->src = rd_string_nonempty(r, val, vpath);
    } else {
      fail_unknown_field(r, key, path);
    }
    ti = tok_skip(r->t, val);
  }
  if (span_absent(e->name))
    fail(ERR_PARSE, path, "element requires a name");
}

static void read_template(const Rd *r, u32 obj_i, const char *path,
                          Submission *sub, SubTemplate *t) {
  u32 n, ti, k;
  memset(t, 0, sizeof *t);
  t->kind = span_none();
  t->summary = span_none();
  if (r->t[obj_i].type != J_OBJ)
    fail(ERR_PARSE, path, "expected an object");
  n = r->t[obj_i].size;
  ti = obj_i + 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    char vpath[192];
    if (rd_str_eq(r, key, "kind")) {
      snprintf(vpath, sizeof vpath, "%s.kind", path);
      t->kind = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "expects")) {
      snprintf(vpath, sizeof vpath, "%s.expects", path);
      read_string_list(r, val, vpath, sub, &t->expects_start,
                       &t->expects_count);
    } else if (rd_str_eq(r, key, "summary")) {
      snprintf(vpath, sizeof vpath, "%s.summary", path);
      t->summary = rd_string_nonempty(r, val, vpath);
    } else {
      fail_unknown_field(r, key, path);
    }
    ti = tok_skip(r->t, val);
  }
  if (span_absent(t->kind))
    fail(ERR_PARSE, path, "template requires a kind");
}

/* Fact shape: triple sugar {s,p,o} or the general form {attrs:{...}} with 2-5
 * named slots, at least one of them element-valued (spec §5). allow_options
 * gates the per-fact options; a retract's fact shape carries none. */
static void read_fact(const Rd *r, u32 obj_i, const char *path, Submission *sub,
                      SubFact *f, int allow_options) {
  u32 n, ti, k;
  int saw_s = 0, saw_p = 0, saw_o = 0, saw_attrs = 0;
  memset(f, 0, sizeof *f);
  f->s = span_none();
  f->p = span_none();
  f->o = span_none();
  f->src = span_none();
  f->status = ST_ASSERTED;
  if (r->t[obj_i].type != J_OBJ)
    fail(ERR_PARSE, path, "expected an object");
  n = r->t[obj_i].size;
  ti = obj_i + 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    char vpath[192];
    if (rd_str_eq(r, key, "s")) {
      snprintf(vpath, sizeof vpath, "%s.s", path);
      f->s = rd_string_nonempty(r, val, vpath);
      saw_s = 1;
    } else if (rd_str_eq(r, key, "p")) {
      snprintf(vpath, sizeof vpath, "%s.p", path);
      f->p = rd_string_nonempty(r, val, vpath);
      saw_p = 1;
    } else if (rd_str_eq(r, key, "o")) {
      snprintf(vpath, sizeof vpath, "%s.o", path);
      f->o = rd_string_nonempty(r, val, vpath);
      saw_o = 1;
    } else if (rd_str_eq(r, key, "attrs")) {
      snprintf(vpath, sizeof vpath, "%s.attrs", path);
      read_attrs(r, val, vpath, sub, &f->attr_start, &f->attr_count);
      saw_attrs = 1;
    } else if (allow_options && rd_str_eq(r, key, "status")) {
      snprintf(vpath, sizeof vpath, "%s.status", path);
      if (rd_str_eq(r, val, "asserted"))
        f->status = ST_ASSERTED;
      else if (rd_str_eq(r, val, "entailed"))
        f->status = ST_ENTAILED;
      else if (rd_str_eq(r, val, "defeasible"))
        f->status = ST_DEFEASIBLE;
      else
        fail(ERR_PARSE, vpath, "expected asserted, entailed, or defeasible");
    } else if (allow_options && rd_str_eq(r, key, "confidence")) {
      snprintf(vpath, sizeof vpath, "%s.confidence", path);
      f->confidence = rd_unit_float(r, val, vpath);
      f->has_confidence = 1;
    } else if (allow_options && rd_str_eq(r, key, "salience")) {
      snprintf(vpath, sizeof vpath, "%s.salience", path);
      f->salience = rd_unit_float(r, val, vpath);
      f->has_salience = 1;
    } else if (allow_options && rd_str_eq(r, key, "src")) {
      snprintf(vpath, sizeof vpath, "%s.src", path);
      f->src = rd_string_nonempty(r, val, vpath);
    } else if (allow_options && rd_str_eq(r, key, "modal")) {
      u32 mn, mk, mti;
      snprintf(vpath, sizeof vpath, "%s.modal", path);
      mn = rd_array(r, val, vpath);
      mti = val + 1;
      for (mk = 0; mk < mn; mk++) {
        if (rd_str_eq(r, mti, "intervened"))
          f->modal |= MODF_INTERVENED;
        else if (rd_str_eq(r, mti, "negated"))
          f->modal |= MODF_NEGATED;
        else if (rd_str_eq(r, mti, "uncertain"))
          f->modal |= MODF_UNCERTAIN;
        else if (rd_str_eq(r, mti, "non_actual"))
          f->modal |= MODF_NON_ACTUAL;
        else if (rd_str_eq(r, mti, "general"))
          f->modal |= MODF_GENERAL;
        else
          fail(ERR_PARSE, vpath,
               "unknown modal (intervened|negated|uncertain|non_actual|general)");
        mti = tok_skip(r->t, mti);
      }
    } else {
      fail_unknown_field(r, key, path);
    }
    ti = tok_skip(r->t, val);
  }
  if (saw_attrs && (saw_s || saw_p || saw_o))
    fail(ERR_PARSE, path, "use either s/p/o or attrs, not both");
  if (saw_attrs) {
    u32 a, m;
    int has_element_ref = 0;
    f->is_triple = 0;
    if (f->attr_count < 2 || f->attr_count > 5)
      fail(ERR_PARSE, path, "a general-form fact takes 2-5 named slots");
    for (a = 0; a < f->attr_count && !has_element_ref; a++) {
      const AttrEntry *e = &sub->attr_pool[f->attr_start + a];
      for (m = 0; m < e->val_count; m++)
        if (!span_is_rel_ref(r->buf, sub->span_pool[e->val_start + m])) {
          has_element_ref = 1;
          break;
        }
    }
    if (!has_element_ref)
      fail(ERR_PARSE, path,
           "slots must include at least one element-valued ref");
  } else {
    if (!(saw_s && saw_p && saw_o))
      fail(ERR_PARSE, path,
           "a fact needs s, p, and o (or a general-form attrs)");
    f->is_triple = 1;
    if (span_is_rel_ref(r->buf, f->s) && span_is_rel_ref(r->buf, f->o))
      fail(ERR_PARSE, path,
           "slots must include at least one element-valued ref");
  }
}

static void read_change(const Rd *r, u32 obj_i, const char *path,
                        SubChange *c) {
  u32 n, ti, k;
  memset(c, 0, sizeof *c);
  c->target = span_none();
  c->property = span_none();
  c->from = span_none();
  c->to = span_none();
  c->event = span_none();
  c->src = span_none();
  if (r->t[obj_i].type != J_OBJ)
    fail(ERR_PARSE, path, "expected an object");
  n = r->t[obj_i].size;
  ti = obj_i + 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    char vpath[192];
    if (rd_str_eq(r, key, "target")) {
      snprintf(vpath, sizeof vpath, "%s.target", path);
      c->target = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "property")) {
      snprintf(vpath, sizeof vpath, "%s.property", path);
      c->property = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "from")) {
      snprintf(vpath, sizeof vpath, "%s.from", path);
      c->from = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "to")) {
      snprintf(vpath, sizeof vpath, "%s.to", path);
      c->to = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "event")) {
      snprintf(vpath, sizeof vpath, "%s.event", path);
      c->event = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "intervened")) {
      snprintf(vpath, sizeof vpath, "%s.intervened", path);
      c->intervened = rd_bool(r, val, vpath);
    } else if (rd_str_eq(r, key, "confidence")) {
      snprintf(vpath, sizeof vpath, "%s.confidence", path);
      c->confidence = rd_unit_float(r, val, vpath);
      c->has_confidence = 1;
    } else if (rd_str_eq(r, key, "src")) {
      snprintf(vpath, sizeof vpath, "%s.src", path);
      c->src = rd_string_nonempty(r, val, vpath);
    } else {
      fail_unknown_field(r, key, path);
    }
    ti = tok_skip(r->t, val);
  }
  if (span_absent(c->target))
    fail(ERR_PARSE, path, "change requires a target");
  if (span_absent(c->property))
    fail(ERR_PARSE, path, "change requires a property");
  if (span_absent(c->to))
    fail(ERR_PARSE, path, "change requires a to value");
}

static void read_retract(const Rd *r, u32 val_i, const char *path,
                         Submission *sub, SubRetract *rt) {
  memset(rt, 0, sizeof *rt);
  if (r->t[val_i].type == J_STR) {
    Span s = rd_span(r, val_i);
    if (!span_is_rel_ref(r->buf, s))
      fail(ERR_PARSE, path, "expected \"rel:<id>\" or a fact shape");
    rt->is_rel_ref = 1;
    rt->rel_ref = s;
    return;
  }
  if (r->t[val_i].type == J_OBJ) {
    read_fact(r, val_i, path, sub, &rt->fact, 0 /* no per-fact options */);
    return;
  }
  fail(ERR_PARSE, path, "expected \"rel:<id>\" or a fact shape");
}

static void read_merge(const Rd *r, u32 obj_i, const char *path, SubMerge *m) {
  u32 n, ti, k;
  m->from = span_none();
  m->into = span_none();
  if (r->t[obj_i].type != J_OBJ)
    fail(ERR_PARSE, path, "expected an object");
  n = r->t[obj_i].size;
  ti = obj_i + 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    char vpath[192];
    if (rd_str_eq(r, key, "from")) {
      snprintf(vpath, sizeof vpath, "%s.from", path);
      m->from = rd_string_nonempty(r, val, vpath);
    } else if (rd_str_eq(r, key, "into")) {
      snprintf(vpath, sizeof vpath, "%s.into", path);
      m->into = rd_string_nonempty(r, val, vpath);
    } else {
      fail_unknown_field(r, key, path);
    }
    ti = tok_skip(r->t, val);
  }
  if (span_absent(m->from))
    fail(ERR_PARSE, path, "merge requires a from ref");
  if (span_absent(m->into))
    fail(ERR_PARSE, path, "merge requires an into ref");
}

static void read_intent(const Rd *r, u32 obj_i, double out[4]) {
  u32 n, ti, k;
  if (r->t[obj_i].type != J_OBJ)
    fail(ERR_PARSE, "intent", "expected an object");
  n = r->t[obj_i].size;
  ti = obj_i + 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    if (rd_str_eq(r, key, "conviction"))
      out[INTENT_CONVICTION] = rd_unit_float(r, val, "intent.conviction");
    else if (rd_str_eq(r, key, "prediction_error"))
      out[INTENT_PREDICTION_ERROR] =
          rd_unit_float(r, val, "intent.prediction_error");
    else if (rd_str_eq(r, key, "arousal"))
      out[INTENT_AROUSAL] = rd_unit_float(r, val, "intent.arousal");
    else if (rd_str_eq(r, key, "curiosity"))
      out[INTENT_CURIOSITY] = rd_unit_float(r, val, "intent.curiosity");
    else
      fail_unknown_field(r, key, "intent");
    ti = tok_skip(r->t, val);
  }
}

/* Reads the whole submission from token 0. *sub may be reused across calls;
 * counts reset here, allocations are kept. */
static void read_submission(const Rd *r, Submission *sub) {
  u32 n, ti, k;
  sub->source = span_none();
  sub->element_count = sub->template_count = sub->fact_count = 0;
  sub->change_count = sub->retract_count = sub->merge_count = 0;
  sub->focus_count = 0;
  sub->span_pool_count = 0;
  sub->attr_pool_count = 0;
  sub->observe = 0;
  memcpy(sub->intent, INTENT_DEFAULTS_SAVE, sizeof sub->intent);
  if (r->t[0].type != J_OBJ)
    fail(ERR_PARSE, NULL, "payload must be a JSON object");
  n = r->t[0].size;
  ti = 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    if (rd_str_eq(r, key, "source")) {
      sub->source = rd_string_nonempty(r, val, "source");
    } else if (rd_str_eq(r, key, "elements")) {
      u32 c = rd_array(r, val, "elements");
      u32 ei = val + 1, m;
      sub->elements = (SubElement *)xgrow(sub->elements, c, &sub->element_cap,
                                          sizeof *sub->elements);
      for (m = 0; m < c; m++) {
        char path[64];
        snprintf(path, sizeof path, "elements[%u]", m);
        read_element(r, ei, path, sub, &sub->elements[m]);
        ei = tok_skip(r->t, ei);
      }
      sub->element_count = c;
    } else if (rd_str_eq(r, key, "templates")) {
      u32 c = rd_array(r, val, "templates");
      u32 ei = val + 1, m;
      sub->templates = (SubTemplate *)xgrow(
          sub->templates, c, &sub->template_cap, sizeof *sub->templates);
      for (m = 0; m < c; m++) {
        char path[64];
        snprintf(path, sizeof path, "templates[%u]", m);
        read_template(r, ei, path, sub, &sub->templates[m]);
        ei = tok_skip(r->t, ei);
      }
      sub->template_count = c;
    } else if (rd_str_eq(r, key, "facts")) {
      u32 c = rd_array(r, val, "facts");
      u32 ei = val + 1, m;
      sub->facts =
          (SubFact *)xgrow(sub->facts, c, &sub->fact_cap, sizeof *sub->facts);
      for (m = 0; m < c; m++) {
        char path[64];
        snprintf(path, sizeof path, "facts[%u]", m);
        read_fact(r, ei, path, sub, &sub->facts[m], 1);
        ei = tok_skip(r->t, ei);
      }
      sub->fact_count = c;
    } else if (rd_str_eq(r, key, "changes")) {
      u32 c = rd_array(r, val, "changes");
      u32 ei = val + 1, m;
      sub->changes = (SubChange *)xgrow(sub->changes, c, &sub->change_cap,
                                        sizeof *sub->changes);
      for (m = 0; m < c; m++) {
        char path[64];
        snprintf(path, sizeof path, "changes[%u]", m);
        read_change(r, ei, path, &sub->changes[m]);
        ei = tok_skip(r->t, ei);
      }
      sub->change_count = c;
    } else if (rd_str_eq(r, key, "retract")) {
      u32 c = rd_array(r, val, "retract");
      u32 ei = val + 1, m;
      sub->retracts = (SubRetract *)xgrow(sub->retracts, c, &sub->retract_cap,
                                          sizeof *sub->retracts);
      for (m = 0; m < c; m++) {
        char path[64];
        snprintf(path, sizeof path, "retract[%u]", m);
        read_retract(r, ei, path, sub, &sub->retracts[m]);
        ei = tok_skip(r->t, ei);
      }
      sub->retract_count = c;
    } else if (rd_str_eq(r, key, "merge")) {
      u32 c = rd_array(r, val, "merge");
      u32 ei = val + 1, m;
      sub->merges = (SubMerge *)xgrow(sub->merges, c, &sub->merge_cap,
                                      sizeof *sub->merges);
      for (m = 0; m < c; m++) {
        char path[64];
        snprintf(path, sizeof path, "merge[%u]", m);
        read_merge(r, ei, path, &sub->merges[m]);
        ei = tok_skip(r->t, ei);
      }
      sub->merge_count = c;
    } else if (rd_str_eq(r, key, "intent")) {
      read_intent(r, val, sub->intent);
    } else if (rd_str_eq(r, key, "focus")) {
      u32 start;
      read_string_list(r, val, "focus", sub, &start, &sub->focus_count);
      {
        u32 m;
        sub->focus = (Span *)xgrow(sub->focus, sub->focus_count,
                                   &sub->focus_cap, sizeof *sub->focus);
        for (m = 0; m < sub->focus_count; m++)
          sub->focus[m] = sub->span_pool[start + m];
      }
    } else if (rd_str_eq(r, key, "observe")) {
      /* observe is a recall flag: a save that writes is not an observation */
      sub->observe = rd_bool(r, val, "observe");
      if (sub->observe)
        fail(ERR_PARSE, "observe", "observe is not accepted on save");
    } else {
      fail_unknown_field(r, key, NULL);
    }
    ti = tok_skip(r->t, val);
  }
  if (sub->element_count + sub->template_count + sub->fact_count +
          sub->change_count + sub->retract_count + sub->merge_count ==
      0)
    fail(ERR_PARSE, NULL,
         "a save needs at least one non-empty write list (elements, templates, "
         "facts, changes, retract, merge)");
}

/* "YYYY-MM-DD" -> unix seconds at UTC midnight. Civil-days arithmetic, no
 * libc time conversion: deterministic and tz-free. */

/* Days since the unix epoch (1970-01-01) for a Gregorian y-m-d. This is Howard
 * Hinnant's well-known closed-form calendar algorithm: it shifts the year so
 * March is month 0 (putting the leap day at the end of the year), groups years
 * into 400-year "eras" (146097 days each, the Gregorian cycle), and computes
 * the day-of-era directly. No loops, no tables, exact for any date. */
static i64 days_from_civil(i64 y, u32 m, u32 d) {
  i64 era;
  u32 yoe, doy, doe;
  y -= m <= 2;
  era = (y >= 0 ? y : y - 399) / 400;
  yoe = (u32)(y - era * 400);
  doy = (153 * (m > 2 ? m - 3 : m + 9) + 2) / 5 + d - 1;
  doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
  return era * 146097 + (i64)doe - 719468;
}

static i64 parse_iso_date(const char *s, u32 len, const char *path) {
  static const u8 MDAYS[12] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
  u32 k, y, m, d, dim;
  int leap;
  if (len != 10 || s[4] != '-' || s[7] != '-')
    fail(ERR_PARSE, path, "expected an ISO date (YYYY-MM-DD)");
  for (k = 0; k < 10; k++) {
    if (k == 4 || k == 7)
      continue;
    if (s[k] < '0' || s[k] > '9')
      fail(ERR_PARSE, path, "expected an ISO date (YYYY-MM-DD)");
  }
  y = (u32)(s[0] - '0') * 1000 + (u32)(s[1] - '0') * 100 +
      (u32)(s[2] - '0') * 10 + (u32)(s[3] - '0');
  m = (u32)(s[5] - '0') * 10 + (u32)(s[6] - '0');
  d = (u32)(s[8] - '0') * 10 + (u32)(s[9] - '0');
  if (m < 1 || m > 12)
    fail(ERR_PARSE, path, "month out of range");
  leap = (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0));
  dim = MDAYS[m - 1];
  if (m == 2 && leap)
    dim = 29;
  if (d < 1 || d > dim)
    fail(ERR_PARSE, path, "day out of range");
  return days_from_civil((i64)y, m, d) * 86400;
}

/* Reads the whole recall payload from token 0. *rec may be reused. */
static void read_recall(const Rd *r, Recall *rec) {
  u32 n, ti, k;
  rec->focus_count = 0;
  rec->query.off = 0;
  rec->query.len = 0;
  rec->limit = 40;
  rec->history_depth = 2;
  rec->since = -1;
  memcpy(rec->intent, INTENT_DEFAULTS_RECALL, sizeof rec->intent);
  rec->observe = 0;
  if (r->t[0].type != J_OBJ)
    fail(ERR_PARSE, NULL, "payload must be a JSON object");
  n = r->t[0].size;
  ti = 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    if (rd_str_eq(r, key, "focus")) {
      u32 c = rd_array(r, val, "focus");
      u32 ei = val + 1, m;
      rec->focus =
          (Span *)xgrow(rec->focus, c, &rec->focus_cap, sizeof *rec->focus);
      for (m = 0; m < c; m++) {
        char path[64];
        snprintf(path, sizeof path, "focus[%u]", m);
        rec->focus[m] = rd_string_nonempty(r, ei, path);
        ei = tok_skip(r->t, ei);
      }
      rec->focus_count = c;
    } else if (rd_str_eq(r, key, "limit")) {
      if (r->t[val].type == J_NULL) {
        rec->limit = -1; /* uncapped */
      } else {
        i64 v = rd_integer(r, val, "limit");
        if (v < 1)
          fail(ERR_PARSE, "limit", "expected a positive integer or null");
        /* anything above the relation id space means "everything" —
         * clamp so budget arithmetic never wraps a u32 */
        rec->limit = v > 0x7FFFFFFF ? -1 : v;
      }
    } else if (rd_str_eq(r, key, "history_depth")) {
      if (r->t[val].type == J_NULL) {
        rec->history_depth = -1; /* full chains */
      } else {
        i64 v = rd_integer(r, val, "history_depth");
        if (v < 0 || v > 0x7FFFFFFF)
          fail(ERR_PARSE, "history_depth",
               "expected a non-negative integer or null");
        rec->history_depth = (i32)v;
      }
    } else if (rd_str_eq(r, key, "since")) {
      Span s = rd_string(r, val, "since");
      rec->since = parse_iso_date(r->buf + s.off, s.len, "since");
    } else if (rd_str_eq(r, key, "query")) {
      rec->query = rd_string(r, val, "query");
    } else if (rd_str_eq(r, key, "intent")) {
      read_intent(r, val, rec->intent);
    } else if (rd_str_eq(r, key, "observe")) {
      rec->observe = rd_bool(r, val, "observe");
    } else {
      fail_unknown_field(r, key, NULL);
    }
    ti = tok_skip(r->t, val);
  }
}

/* =============================== S3 — model ==============================
 *
 * The in-memory graph and everything hung off it. Two halves:
 *
 *  PERSISTED STATE (what the snapshot on disk holds): the string arena, the
 *  Element and Relation arenas, the tick clock, the tick->wall-clock stamp
 *  table, and the Policy (tuning constants). This is the whole truth of the
 *  store.
 *
 *  DERIVED INDICES (rebuilt from the persisted state on every load, never
 *  saved): lookup tables that make questions fast — "which elements share this
 *  normalized name?", "which relations mention this element?", "does a
 *  relation with this exact slot set already exist?", "what is the current
 *  value of jump_height?". Because they are derived, a corrupt or foreign
 *  snapshot can never carry a bad index in; the reader validates the raw data
 *  and rebuild_indices() recomputes all of this.
 *
 * The status lattice (Asserted/Entailed/Defeasible/Superseded/Retracted) is
 * how a relation's life is tracked: the first three are "live" and appear in
 * frames; Superseded shows only in history; Retracted never shows at all.
 * Supersession and retraction FLIP status, they never delete.
 */

/* Relation status lattice (spec §3): the first three continue the S2 reader
 * enum; the last two are substrate-only and never arrive in a payload. */
enum { ST_SUPERSEDED = 3, ST_RETRACTED = 4 };

/* A Term is one relation slot's VALUE: a tagged union pointing at either an
 * element (tag TERM_ELEM) or another relation (tag TERM_REL, how metas like
 * `source` target a fact). An Attr is one named slot: `name` is the element id
 * of the slot label ("subject", "uses", ...), `value` is the Term it points
 * at. A relation is just an array of these. */
enum { TERM_ELEM = 0, TERM_REL = 1 };

/* Extended-vocabulary index enum (names + rationale at EXT_NAMES, below the
 * legacy WK table). Declared here so the Hypergraph struct can size g->wk_ext. */
enum {
  EXT_CAUSED = 0,
  EXT_CORRELATED_WITH,
  EXT_ENABLES,
  EXT_PREVENTS,
  EXT_SUBCLASS_OF,
  EXT_ANTECEDENT_OF,
  EXT_NEGATED,
  EXT_UNCERTAIN,
  EXT_NON_ACTUAL,
  EXT_GENERAL,
  EXT_COUNT
};

typedef struct {
  u8 tag;
  u32 id;
} Term;
typedef struct {
  u32 name;
  Term value;
} Attr; /* name is an element id */

/* The live stat set carried by every element and relation (spec §3, plan §8
 * M1 freeze checklist). These are the numbers S7's dynamics read and write;
 * only `activation` decays — truth (confidence/status) never does. */
typedef struct {
  double confidence; /* how trusted [0,1]; supersession gate + frame */
  double activation; /* how "warm"/available [0,1]; bumped on touch, decays with
                        neglect */
  double salience;   /* how important [0,1]; seeded by write mechanics, divides
                        decay */
  double stability;  /* resistance to decay (>=1); grows more for spaced than
                        massed touches */
  u32 access_count;  /* times touched; feeds the spacing signal */
  u32 focus_success_count; /* times reached via a focus walk; one half of
                              `related` ranking */
  u32 support_count;       /* dedup re-assertions; >=3 is one promotion gate */
  u32 support_diversity;   /* count of DISTINCT sources; >=2 is the other
                              promotion gate */
  u32 last_seen;
      /* tick */ /* tick of last touch; recency for ranking + spacing */
} Stats;

typedef struct {
  U32Vec names;   /* string ids; names.v[0] is canonical, the rest aliases */
  u32 summary;    /* string id or NONE_U32 */
  u32 redirect;   /* redirect tombstone: element id or NONE_U32 = live */
  u32 created_at; /* tick */
  Stats stats;
} Element;

typedef struct {
  Attr attrs[5];
  u8 attr_count; /* 2..5 */
  u8 status;
  u32 created_at;
  Stats stats;
  U32Vec supporters; /* distinct source element ids; feeds support_diversity */
} Relation;

/* Dynamics rates + intent-consumed scalars (spec §3.1/§10, plan §3.5/§3.6).
 * Persisted in the snapshot policy block; defaults are provisional shapes the
 * corpus tunes. */
typedef struct {
  double hebbian_rate;           /* 0.1 */
  double decay_rate;             /* 0.02 */
  double stability_spaced;       /* x1.3 on spaced touch */
  double stability_massed;       /* x1.05 on massed touch */
  double stability_cap;          /* tanh cap 20 */
  double default_confidence;     /* 0.7 */
  double supersession_threshold; /* v2 default, ported per pin 6; the changes
                                    gate (M2) */
  double salience_conflict;      /* 0.9  graph-PE seeds, plan §3.5 */
  double salience_supersession;  /* 0.7 */
  double salience_novel;         /* 0.5 */
  double salience_reuse_bump;    /* 0.05 */
  u32 focus_decay_radius;        /* 2 */
  u32 promotion_min_support;     /* 3 */
  u32 promotion_min_diversity;   /* 2 */
} Policy;

static Policy policy_defaults(void) {
  Policy p;
  p.hebbian_rate = 0.1;
  p.decay_rate = 0.02;
  p.stability_spaced = 1.3;
  p.stability_massed = 1.05;
  p.stability_cap = 20.0;
  p.default_confidence = 0.7;
  p.supersession_threshold = 0.35;
  p.salience_conflict = 0.9;
  p.salience_supersession = 0.7;
  p.salience_novel = 0.5;
  p.salience_reuse_bump = 0.05;
  p.focus_decay_radius = 2;
  p.promotion_min_support = 3;
  p.promotion_min_diversity = 2;
  return p;
}

/* Mint-time overflow guards (plan §3.11) -> store_full. Mutable so tests can
 * shrink them; NONE_U32 stays reserved as the handle sentinel. */
static u32 g_cap_elements = 0xFFFFFFFEu;
static u32 g_cap_relations = 0xFFFFFFFEu;
static u32 g_cap_strings = 0xFFFFFFFEu;
static u32 g_cap_ticks = 0xFFFFFFFEu;

typedef struct {
  u32 elem;
  u32 next;
} NameLink; /* by_norm chain node */
typedef struct {
  u32 rel;
  u32 next;
} RelLink; /* rels_by_elem / meta_by_target node */

typedef struct {
  u32 target, name_elem, rel;
} CurEntry; /* current_* cache slot */

typedef struct {
  /* ---- persisted state ---- */
  StrArena strs; /* every caller-visible string: names, summaries, sources */
  Element *elements;
  u32 element_count, element_cap;
  Relation *relations;
  u32 relation_count, relation_cap;
  u32 clock; /* one tick per non-observe invocation */
  u64 *stamps;
  u32 stamp_cap; /* stamps[i] = wall seconds of tick i+1; count == clock */
  Policy policy;

  /* ---- derived indices, rebuilt from the arenas, never persisted ----
   * Each answers one lookup the write path and frame assembly need fast.
   * name_links and rel_links are shared node pools: the maps/arrays below
   * store a HEAD index into them, and each node's `next` chains the rest
   * (a hand-rolled linked list, since realloc forbids real pointers). */
  StrArena norms; /* interned normalized name forms (the tier-1 match keys) */
  StrMap by_norm; /* "which elements have this normalized name?" -> NameLink
                     chain head */
  NameLink *name_links;
  u32 name_link_count, name_link_cap; /* by_norm chain nodes */
  RelLink *rel_links;
  u32 rel_link_count,
      rel_link_cap;  /* rels_by_elem / meta_by_target chain nodes */
  u32 *rels_by_elem; /* "which relations have this element in a slot?" ->
                        RelLink head (per element) */
  u32 rels_by_elem_cap;
  u32 *meta_by_target; /* "which meta-relations point AT this relation?" ->
                          RelLink head (per relation) */
  u32 meta_by_target_cap;
  u32 *elem_kind; /* "what kind is this element?" -> kind element id (latest live
                     instance_of), per element */
  u32 elem_kind_cap;
  u32 *dedup_slots; /* "does a relation with this exact slot set already exist?"
                       attr-set hash; slot = rel id + 1, 0 = empty */
  u32 dedup_cap, dedup_used;
  CurEntry *cur_slots; /* "what is the current value of (target, property)?" ->
                          cache rel; target == NONE_U32 = empty */
  u32 cur_cap, cur_used;
  u32 wk_ext[EXT_COUNT]; /* resolved element id of each extended-vocab name in
                            THIS store; set by seed_ext_vocab at init and load,
                            never a raw enum id (see EXT_NAMES) */
  u32 wk_self;           /* resolved element id of SELF_NAME in THIS store; set
                            by seed_self at init and load (see SELF_NAME) */
} Hypergraph;

static void graph_free(Hypergraph *g) {
  u32 i;
  for (i = 0; i < g->element_count; i++)
    free(g->elements[i].names.v);
  for (i = 0; i < g->relation_count; i++)
    free(g->relations[i].supporters.v);
  free(g->elements);
  free(g->relations);
  free(g->stamps);
  str_arena_free(&g->strs);
  str_arena_free(&g->norms);
  smap_free(&g->by_norm);
  free(g->name_links);
  free(g->rel_links);
  free(g->rels_by_elem);
  free(g->meta_by_target);
  free(g->elem_kind);
  free(g->dedup_slots);
  free(g->cur_slots);
  memset(g, 0, sizeof *g);
}

static u32 graph_intern(Hypergraph *g, const char *s, u32 len) {
  u32 id = str_find(&g->strs, s, len);
  if (id != NONE_U32)
    return id;
  if (g->strs.count >= g_cap_strings)
    fail(ERR_STORE_FULL, NULL, "string arena is full (%u strings)",
         g->strs.count);
  return str_intern(&g->strs, s, len);
}

/* Normalization scratch, grown to fit; reused across calls. */
static char *g_norm_buf;
static u32 g_norm_cap;

static u32 normalize_into_scratch(const char *s, u32 len) {
  g_norm_buf = (char *)xgrow(g_norm_buf, len ? len : 1, &g_norm_cap, 1);
  return normalize_name(s, len, g_norm_buf);
}

/* by_norm multimap: normalized form -> chain of element ids. */
static void index_norm_name(Hypergraph *g, u32 elem, const char *name,
                            u32 len) {
  u32 nlen = normalize_into_scratch(name, len);
  u32 nid, head;
  if (nlen == 0)
    return; /* plan and the snapshot reader both reject these upstream */
  nid = str_intern(&g->norms, g_norm_buf, nlen);
  head = smap_get(&g->by_norm, &g->norms, g_norm_buf, nlen);
  g->name_links = (NameLink *)xgrow(g->name_links, g->name_link_count + 1,
                                    &g->name_link_cap, sizeof *g->name_links);
  g->name_links[g->name_link_count].elem = elem;
  g->name_links[g->name_link_count].next = head; /* NONE_U32 terminates */
  smap_put(&g->by_norm, &g->norms, nid, g->name_link_count);
  g->name_link_count++;
}

static u32 rel_link_push(Hypergraph *g, u32 rel, u32 head) {
  g->rel_links = (RelLink *)xgrow(g->rel_links, g->rel_link_count + 1,
                                  &g->rel_link_cap, sizeof *g->rel_links);
  g->rel_links[g->rel_link_count].rel = rel;
  g->rel_links[g->rel_link_count].next = head;
  return g->rel_link_count++;
}

/* Attribute-set canonical form: the (name, tag, id) triples sorted ascending.
 * Set equality ignores slot order; multi-valued payload slots were already
 * expanded to one relation per value upstream. */
static void attr_set_canon(const Attr *attrs, u8 count, Attr out[5]) {
  u8 i, j;
  for (i = 0; i < count; i++)
    out[i] = attrs[i];
  for (i = 1; i < count; i++) {
    Attr key = out[i];
    for (j = i; j > 0; j--) {
      const Attr *p = &out[j - 1];
      if (p->name < key.name)
        break;
      if (p->name == key.name) {
        if (p->value.tag < key.value.tag)
          break;
        if (p->value.tag == key.value.tag && p->value.id <= key.value.id)
          break;
      }
      out[j] = out[j - 1];
    }
    out[j] = key;
  }
}

static u32 attr_set_hash(const Attr *attrs, u8 count) {
  Attr canon[5];
  u8 buf[5 * 9];
  u8 i;
  attr_set_canon(attrs, count, canon);
  for (i = 0; i < count; i++) {
    u8 *p = buf + (size_t)i * 9;
    u32 n = canon[i].name, v = canon[i].value.id;
    p[0] = (u8)n;
    p[1] = (u8)(n >> 8);
    p[2] = (u8)(n >> 16);
    p[3] = (u8)(n >> 24);
    p[4] = canon[i].value.tag;
    p[5] = (u8)v;
    p[6] = (u8)(v >> 8);
    p[7] = (u8)(v >> 16);
    p[8] = (u8)(v >> 24);
  }
  return fnv1a((const char *)buf, (u32)count * 9);
}

static int attr_set_eq(const Attr *a, u8 an, const Attr *b, u8 bn) {
  Attr ca[5], cb[5];
  u8 i;
  if (an != bn)
    return 0;
  attr_set_canon(a, an, ca);
  attr_set_canon(b, bn, cb);
  for (i = 0; i < an; i++)
    if (ca[i].name != cb[i].name || ca[i].value.tag != cb[i].value.tag ||
        ca[i].value.id != cb[i].value.id)
      return 0;
  return 1;
}

static void dedup_grow(Hypergraph *g, u32 new_cap) {
  u32 *old = g->dedup_slots;
  u32 old_cap = g->dedup_cap, i;
  g->dedup_slots = (u32 *)calloc(new_cap, sizeof *g->dedup_slots);
  if (!g->dedup_slots) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  g->dedup_cap = new_cap;
  g->dedup_used = 0;
  for (i = 0; i < old_cap; i++) {
    if (old[i]) {
      u32 rid = old[i] - 1;
      const Relation *r = &g->relations[rid];
      u32 mask = new_cap - 1;
      u32 s = attr_set_hash(r->attrs, r->attr_count) & mask;
      while (g->dedup_slots[s])
        s = (s + 1) & mask;
      g->dedup_slots[s] = rid + 1;
      g->dedup_used++;
    }
  }
  free(old);
}

static void dedup_insert(Hypergraph *g, u32 rel_id) {
  const Relation *r = &g->relations[rel_id];
  u32 mask, i;
  if (g->dedup_cap == 0 || g->dedup_used + 1 > g->dedup_cap - g->dedup_cap / 4)
    dedup_grow(g, g->dedup_cap ? g->dedup_cap * 2 : 16);
  mask = g->dedup_cap - 1;
  i = attr_set_hash(r->attrs, r->attr_count) & mask;
  while (g->dedup_slots[i])
    i = (i + 1) & mask;
  g->dedup_slots[i] = rel_id + 1;
  g->dedup_used++;
}

/* First live relation with this exact attr set, or NONE_U32. Live =
 * Asserted/Entailed/Defeasible: Retracted never re-participates (spec §5)
 * and neither does Superseded — priors are never revived, so a rolled-back
 * value mints a fresh cache instead of resurrecting the superseded one. */
static u32 dedup_lookup_live(const Hypergraph *g, const Attr *attrs, u8 count) {
  u32 mask, i;
  if (g->dedup_cap == 0)
    return NONE_U32;
  mask = g->dedup_cap - 1;
  i = attr_set_hash(attrs, count) & mask;
  while (g->dedup_slots[i]) {
    u32 rid = g->dedup_slots[i] - 1;
    const Relation *r = &g->relations[rid];
    if (r->status < ST_SUPERSEDED &&
        attr_set_eq(attrs, count, r->attrs, r->attr_count))
      return rid;
    i = (i + 1) & mask;
  }
  return NONE_U32;
}

static void cur_grow(Hypergraph *g, u32 new_cap) {
  CurEntry *old = g->cur_slots;
  u32 old_cap = g->cur_cap, i;
  g->cur_slots = (CurEntry *)malloc((size_t)new_cap * sizeof *g->cur_slots);
  if (!g->cur_slots) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  for (i = 0; i < new_cap; i++)
    g->cur_slots[i].target = NONE_U32;
  g->cur_cap = new_cap;
  g->cur_used = 0;
  for (i = 0; i < old_cap; i++) {
    if (old[i].target != NONE_U32) {
      u32 mask = new_cap - 1;
      u32 h = fnv1a((const char *)&old[i], 8); /* target + name_elem */
      u32 s = h & mask;
      while (g->cur_slots[s].target != NONE_U32)
        s = (s + 1) & mask;
      g->cur_slots[s] = old[i];
      g->cur_used++;
    }
  }
  free(old);
}

/* current_* cache index, keyed (target, current_<property>-name element) —
 * a bijection of (target, property) given plan §3.3's literal naming rule. */
static void cur_put(Hypergraph *g, u32 target, u32 name_elem, u32 rel) {
  CurEntry e;
  u32 mask, i;
  e.target = target;
  e.name_elem = name_elem;
  e.rel = rel;
  if (g->cur_cap == 0 || g->cur_used + 1 > g->cur_cap - g->cur_cap / 4)
    cur_grow(g, g->cur_cap ? g->cur_cap * 2 : 16);
  mask = g->cur_cap - 1;
  i = fnv1a((const char *)&e, 8) & mask;
  while (g->cur_slots[i].target != NONE_U32) {
    if (g->cur_slots[i].target == target &&
        g->cur_slots[i].name_elem == name_elem) {
      g->cur_slots[i].rel = rel;
      return;
    }
    i = (i + 1) & mask;
  }
  g->cur_slots[i] = e;
  g->cur_used++;
}

LEGEND_UNUSED static u32 cur_get(const Hypergraph *g, u32 target,
                                 u32 name_elem) {
  CurEntry probe;
  u32 mask, i;
  if (g->cur_cap == 0)
    return NONE_U32;
  probe.target = target;
  probe.name_elem = name_elem;
  probe.rel = 0;
  mask = g->cur_cap - 1;
  i = fnv1a((const char *)&probe, 8) & mask;
  while (g->cur_slots[i].target != NONE_U32) {
    if (g->cur_slots[i].target == target &&
        g->cur_slots[i].name_elem == name_elem)
      return g->cur_slots[i].rel;
    i = (i + 1) & mask;
  }
  return NONE_U32;
}

/* Arena pushes: zero the new record and grow the parallel index arrays in
 * lockstep so graph_free is safe at any intermediate point. */
static u32 element_arena_push(Hypergraph *g) {
  u32 id = g->element_count;
  if (id >= g_cap_elements)
    fail(ERR_STORE_FULL, NULL, "element arena is full (%u elements)", id);
  g->elements = (Element *)xgrow(g->elements, id + 1, &g->element_cap,
                                 sizeof *g->elements);
  memset(&g->elements[id], 0, sizeof g->elements[id]);
  g->elements[id].summary = NONE_U32;
  g->elements[id].redirect = NONE_U32;
  g->rels_by_elem = (u32 *)xgrow(g->rels_by_elem, id + 1, &g->rels_by_elem_cap,
                                 sizeof *g->rels_by_elem);
  g->rels_by_elem[id] = NONE_U32;
  g->elem_kind = (u32 *)xgrow(g->elem_kind, id + 1, &g->elem_kind_cap,
                              sizeof *g->elem_kind);
  g->elem_kind[id] = NONE_U32;
  g->element_count = id + 1;
  return id;
}

static u32 relation_arena_push(Hypergraph *g) {
  u32 id = g->relation_count;
  if (id >= g_cap_relations)
    fail(ERR_STORE_FULL, NULL, "relation arena is full (%u relations)", id);
  g->relations = (Relation *)xgrow(g->relations, id + 1, &g->relation_cap,
                                   sizeof *g->relations);
  memset(&g->relations[id], 0, sizeof g->relations[id]);
  g->meta_by_target =
      (u32 *)xgrow(g->meta_by_target, id + 1, &g->meta_by_target_cap,
                   sizeof *g->meta_by_target);
  g->meta_by_target[id] = NONE_U32;
  g->relation_count = id + 1;
  return id;
}

static Stats stats_new(double confidence, double salience, u32 tick) {
  Stats s;
  s.confidence = confidence;
  s.activation = 0.0;
  s.salience = salience;
  s.stability = 1.0;
  s.access_count = 0;
  s.focus_success_count = 0;
  s.support_count = 0;
  s.support_diversity = 0;
  s.last_seen = tick;
  return s;
}

/* Slot the relation into every derived index. Shared by mint and the
 * snapshot-load rebuild, so the two paths cannot diverge. Besides the generic
 * rels_by_elem/meta_by_target/dedup wiring, it reads the slot NAMES to
 * maintain two special indices: it finds the "subject" element, then if a slot
 * is "instance_of" records the subject's kind, and if a slot name begins with
 * "current_" registers the subject's current-value cache (only for live
 * relations). String lengths are checked before memcmp so the compares are
 * cheap. */
static void index_relation(Hypergraph *g, u32 rid) {
  const Relation *r = &g->relations[rid];
  int live = r->status < ST_SUPERSEDED;
  u32 a;
  u32 subject_elem = NONE_U32;
  dedup_insert(g, rid);
  for (a = 0; a < r->attr_count; a++) {
    if (r->attrs[a].value.tag == TERM_ELEM)
      g->rels_by_elem[r->attrs[a].value.id] =
          rel_link_push(g, rid, g->rels_by_elem[r->attrs[a].value.id]);
    else
      g->meta_by_target[r->attrs[a].value.id] =
          rel_link_push(g, rid, g->meta_by_target[r->attrs[a].value.id]);
  }
  for (a = 0; a < r->attr_count; a++) {
    u32 name_id = g->elements[r->attrs[a].name].names.v[0];
    if (str_len(&g->strs, name_id) == 7 &&
        memcmp(str_ptr(&g->strs, name_id), "subject", 7) == 0 &&
        r->attrs[a].value.tag == TERM_ELEM)
      subject_elem = r->attrs[a].value.id;
  }
  if (!live)
    return;
  for (a = 0; a < r->attr_count; a++) {
    u32 name_elem = r->attrs[a].name;
    u32 name_id = g->elements[name_elem].names.v[0];
    const char *nb = str_ptr(&g->strs, name_id);
    u32 nl = str_len(&g->strs, name_id);
    if (nl == 11 && memcmp(nb, "instance_of", 11) == 0 &&
        r->attrs[a].value.tag == TERM_ELEM && subject_elem != NONE_U32)
      /* latest LIVE instance_of wins: this block runs only for live relations
       * (see !live guard above), and a kind correction supersedes the old
       * instance_of, so on load exactly one is live. Last-in-arena == last
       * minted, so warm accumulation and cold reload agree. */
      g->elem_kind[subject_elem] = r->attrs[a].value.id;
    if (nl > 8 && memcmp(nb, "current_", 8) == 0 && subject_elem != NONE_U32)
      cur_put(g, subject_elem, name_elem, rid);
  }
}

static u32 mint_relation(Hypergraph *g, const Attr *attrs, u8 count, u8 status,
                         double confidence, double salience, u32 tick,
                         u32 source_elem) {
  u32 id = relation_arena_push(g);
  Relation *r = &g->relations[id];
  memcpy(r->attrs, attrs, (size_t)count * sizeof *attrs);
  r->attr_count = count;
  r->status = status;
  r->created_at = tick;
  r->stats = stats_new(confidence, salience, tick);
  r->stats.support_count = 1;
  if (source_elem != NONE_U32) {
    u32vec_push(&r->supporters, source_elem);
    r->stats.support_diversity = 1;
  }
  index_relation(g, id);
  return id;
}

static u32 mint_element(Hypergraph *g, const char *name, u32 len,
                        double confidence, double salience, u32 tick) {
  u32 sid = graph_intern(g, name, len);
  u32 id = element_arena_push(g);
  Element *e = &g->elements[id];
  u32vec_push(&e->names, sid);
  e->created_at = tick;
  e->stats = stats_new(confidence, salience, tick);
  index_norm_name(g, id, name, len);
  return id;
}

/* The cur index entry for (target, current_* name), but only while the cache
 * relation is still live — a retracted cache leaves a stale slot behind, and
 * a stale prior must read as "no prior" (spec §5: priors are never revived). */
static u32 cur_get_live(const Hypergraph *g, u32 target, u32 name_elem) {
  u32 rel = cur_get(g, target, name_elem);
  if (rel == NONE_U32 || g->relations[rel].status >= ST_SUPERSEDED)
    return NONE_U32;
  return rel;
}

/* Drop and rebuild every derived index from the arenas. Shared by the
 * snapshot reader and the merge fold, so the two paths cannot diverge. */
static void rebuild_indices(Hypergraph *g) {
  u32 i, k;
  str_arena_free(&g->norms);
  smap_free(&g->by_norm);
  g->name_link_count = 0;
  g->rel_link_count = 0;
  for (i = 0; i < g->element_count; i++) {
    g->rels_by_elem[i] = NONE_U32;
    g->elem_kind[i] = NONE_U32;
  }
  for (i = 0; i < g->relation_count; i++)
    g->meta_by_target[i] = NONE_U32;
  if (g->dedup_cap)
    memset(g->dedup_slots, 0, (size_t)g->dedup_cap * sizeof *g->dedup_slots);
  g->dedup_used = 0;
  for (i = 0; i < g->cur_cap; i++)
    g->cur_slots[i].target = NONE_U32;
  g->cur_used = 0;
  for (i = 0; i < g->element_count; i++) {
    const Element *e = &g->elements[i];
    for (k = 0; k < e->names.count; k++)
      index_norm_name(g, i, str_ptr(&g->strs, e->names.v[k]),
                      str_len(&g->strs, e->names.v[k]));
  }
  for (i = 0; i < g->relation_count; i++)
    index_relation(g, i);
}

/* ============================= S4 — ontology ==============================
 *
 * The built-in vocabulary every store is born with. "Ontology" here just means
 * the shared words: the slot names (subject, uses' cousins like `about`,
 * `chose`, `reason`...) and the kinds (decision, constraint, question, task,
 * file, commit...) that callers build facts out of. It ships as const tables
 * in code, not a data file, because it is a CONTRACT: if session 1 says
 * `chose` and session 50 says `selected`, cross-session dedup dies. Seeding it
 * identically on every `init` is what lets a store written by one model be
 * read by another. Because the seed order is fixed, these elements always land
 * at the same ids (the WK_* enum), so the rest of the file can name vocabulary
 * by constant instead of looking it up.
 */

/* The core ontology contract (spec §12), minted in this exact order on init.
 * The ids are load-bearing: the write path addresses vocabulary through this
 * enum, and every store lineage shares them (plan §3.10: all templates seed
 * unconditionally so ids never shift per scope).
 *
 * Well-known ids:
 *   elements
 *     #0  subject      #1  target       #2  property     #3  from
 *     #4  to           #5  at           #6  with         #7  instance_of
 *     #8  reason       #9  chose        #10 rejected     #11 about
 *     #12 applies_to   #13 resolves     #14 standing     #15 expects
 *     #16 part_of      #17 source       #18 src          #19 derived_from
 *     #20 supersedes   #21 intervened
 *     #22 decision     #23 constraint   #24 question     #25 task
 *     #26 pointer      #27 project      #28 person       #29 event
 *     #30 file         #31 commit
 *   relations ({subject: <kind>, expects: <attr>} edges)
 *     rel:0..4  decision expects chose, rejected, reason, about, resolves
 *     rel:5..7  constraint expects applies_to, reason, standing
 *     rel:8     question expects about
 *     rel:9     task expects about
 */
enum {
  WK_SUBJECT = 0,
  WK_TARGET,
  WK_PROPERTY,
  WK_FROM,
  WK_TO,
  WK_AT,
  WK_WITH,
  WK_INSTANCE_OF,
  WK_REASON,
  WK_CHOSE,
  WK_REJECTED,
  WK_ABOUT,
  WK_APPLIES_TO,
  WK_RESOLVES,
  WK_STANDING,
  WK_EXPECTS,
  WK_PART_OF,
  WK_SOURCE,
  WK_SRC,
  WK_DERIVED_FROM,
  WK_SUPERSEDES,
  WK_INTERVENED,
  WK_KIND_DECISION,
  WK_KIND_CONSTRAINT,
  WK_KIND_QUESTION,
  WK_KIND_TASK,
  WK_KIND_POINTER,
  WK_KIND_PROJECT,
  WK_KIND_PERSON,
  WK_KIND_EVENT,
  WK_KIND_FILE,
  WK_KIND_COMMIT,
  WK_ELEMENT_COUNT
};

static const char *const WK_NAMES[WK_ELEMENT_COUNT] = {
    "subject",    "target",     "property",    "from",       "to",
    "at",         "with",       "instance_of", "reason",     "chose",
    "rejected",   "about",      "applies_to",  "resolves",   "standing",
    "expects",    "part_of",    "source",      "src",        "derived_from",
    "supersedes", "intervened", "decision",    "constraint", "question",
    "task",       "pointer",    "project",     "person",     "event",
    "file",       "commit"};

/* Extended vocabulary (Pearl / Book-of-Why causal representation, new_foundation
 * §16.3). Unlike the legacy core (ids 0..WK_ELEMENT_COUNT-1, verified by name in
 * the snapshot reader), these do NOT sit at fixed ids: a store seeded before this
 * vocabulary existed already occupies #32+, so the extended names are seeded
 * contiguously on a fresh init but APPENDED on load of an older store. Every use
 * resolves through g->wk_ext[] -- never a raw id.
 *
 *  - caused / correlated_with / enables / prevents: causal-relation predicates.
 *    caused/enables/prevents carry a rung-2 causal claim; correlated_with is
 *    rung-1 co-occurrence. Kept distinct so retrieval never promotes correlation
 *    to causation (the core Book-of-Why invariant).
 *  - subclass_of: taxonomy / cone membership.  antecedent_of: conditional shape
 *    ("Y holds if X") -- substrate for later forward-chaining / counterfactuals.
 *  - negated / uncertain / non_actual / general: behavioral modals reified as
 *    meta-relations on a claim (intervened is legacy #21).
 *
 * The EXT_* index enum lives before the Hypergraph struct (g->wk_ext sizing). */
static const char *const EXT_NAMES[EXT_COUNT] = {
    "caused",   "correlated_with", "enables",    "prevents", "subclass_of",
    "antecedent_of", "negated",    "uncertain",  "non_actual", "general"};

typedef struct {
  u8 kind;             /* WK_KIND_* */
  const char *summary; /* the kind element's one prose slot */
  u8 expects[5];       /* WK_* attribute vocabulary */
  u8 expects_count;
} SeedTemplate;

static const SeedTemplate WK_TEMPLATES[] = {
    {WK_KIND_DECISION,
     "a choice made between alternatives, with the reasons and what it settles",
     {WK_CHOSE, WK_REJECTED, WK_REASON, WK_ABOUT, WK_RESOLVES},
     5},
    {WK_KIND_CONSTRAINT,
     "a standing rule that must hold while active",
     {WK_APPLIES_TO, WK_REASON, WK_STANDING, 0, 0},
     3},
    {WK_KIND_QUESTION,
     "an open question; a live resolves relation closes it",
     {WK_ABOUT, 0, 0, 0, 0},
     1},
    {WK_KIND_TASK,
     "a unit of work; a live resolves relation closes it",
     {WK_ABOUT, 0, 0, 0, 0},
     1},
    {WK_KIND_POINTER,
     "a pointer to source material: a file location, commit, URL, or "
     "conversation",
     {0, 0, 0, 0, 0},
     0},
    {WK_KIND_PROJECT,
     "a project or scope this store describes",
     {0, 0, 0, 0, 0},
     0},
    {WK_KIND_PERSON, "a person", {0, 0, 0, 0, 0}, 0},
    {WK_KIND_EVENT,
     "something that happened at a point in time",
     {0, 0, 0, 0, 0},
     0},
    {WK_KIND_FILE, "a file in a codebase", {0, 0, 0, 0, 0}, 0},
    {WK_KIND_COMMIT, "a version-control commit", {0, 0, 0, 0, 0}, 0}};

/* rel:0..9 are the seed {subject:<kind>, expects:<attr>} relations — the sum
 * of the WK_TEMPLATES expects counts (decision 5, constraint 3, question 1,
 * task 1). They are the ontology contract, not claims, so they can never be
 * retracted (spec §12). */
enum { WK_RELATION_COUNT = 5 + 3 + 1 + 1 };

/* Seed a fresh graph: vocabulary + templates at tick 0, confidence 1.0 (the
 * ontology is the contract, not a claim), then the shipped policy defaults. */
static void ontology_seed(Hypergraph *g) {
  u32 i, t;
  g->policy = policy_defaults();
  for (i = 0; i < WK_ELEMENT_COUNT; i++)
    mint_element(g, WK_NAMES[i], (u32)strlen(WK_NAMES[i]), 1.0, 0.0, 0);
  for (t = 0; t < sizeof WK_TEMPLATES / sizeof WK_TEMPLATES[0]; t++) {
    const SeedTemplate *st = &WK_TEMPLATES[t];
    u32 e;
    g->elements[st->kind].summary =
        graph_intern(g, st->summary, (u32)strlen(st->summary));
    for (e = 0; e < st->expects_count; e++) {
      Attr attrs[2];
      attrs[0].name = WK_SUBJECT;
      attrs[0].value.tag = TERM_ELEM;
      attrs[0].value.id = st->kind;
      attrs[1].name = WK_EXPECTS;
      attrs[1].value.tag = TERM_ELEM;
      attrs[1].value.id = st->expects[e];
      mint_relation(g, attrs, 2, ST_ASSERTED, 1.0, 0.0, 0, NONE_U32);
    }
  }
}

/* ======================= S5 — resolution (tier 1) ========================
 *
 * RESOLUTION = turning a ref in a payload ("#87", "rel:5", or a name string
 * like "jump feel") into an element (or relation) id. This is where a cold
 * caller's naming drift gets absorbed (spec §8). Two things decide how hard we
 * try: the ref's FORM and its POSITION ROLE.
 *
 *  Forms: "#<n>" is an exact element id (unknown id = hard error; a merged-away
 *  id follows its redirect tombstone). "rel:<n>" is a relation id. Anything
 *  else is a NAME, matched through tiers.
 *
 *  Tiers: tier 1 is normalized-exact — after normalize_name() collapses case
 *  and punctuation, do the keys match? This is deterministic and safe, so it
 *  is the only tier that ever causes REUSE. Tier 2 is trigram similarity (this
 *  file's fuzzy match); it never silently reuses — on writes it only REPORTS a
 *  near-match for the caller to confirm, and on reads it widens focus. (Tier 3,
 *  embeddings, is Rust-only and absent here.)
 *
 *  Position roles (spec §8):
 *   - WRITE positions (element decls, fact/change slot values): tier-1 unique
 *     match => reuse, else MINT. Tier 2 only reports near_matches.
 *   - READ positions (focus): tier 1, then tier-2 candidates >= 0.6 join the
 *     walk. Never mints.
 *   - PRECISE positions (retract/merge): tier-1 or explicit id ONLY — a status
 *     flip or a fold must never run off a guess. A homonym here is an error
 *     with candidates.
 *
 * A HOMONYM is one normalized name held by several live elements ("Mercury" the
 * planet vs the person). resolve_tier1_n picks one (prefer matching kind, then
 * most recently seen, then highest id) and reports the choice; precise
 * positions refuse to guess. This section is tier 1 + tier 2; tier 3 stays
 * Track-3 work.
 */

/* Exactly "#<decimal>" (plan §3.9). Returns the id via out; 0 = not id-shaped.
 */
static int span_parse_elem_id(const char *buf, Span s, u32 *out) {
  u64 v = 0;
  u32 k;
  if (s.len < 2 || buf[s.off] != '#')
    return 0;
  for (k = 1; k < s.len; k++) {
    char c = buf[s.off + k];
    if (c < '0' || c > '9')
      return 0;
    v = v * 10 + (u64)(c - '0');
    if (v > 0xFFFFFFFFu)
      v = 0xFFFFFFFFu; /* saturate: fails the range check */
  }
  *out = (u32)v;
  return 1;
}

static int span_parse_rel_id(const char *buf, Span s, u32 *out) {
  u64 v = 0;
  u32 k;
  if (!span_is_rel_ref(buf, s))
    return 0;
  for (k = 4; k < s.len; k++) {
    v = v * 10 + (u64)(buf[s.off + k] - '0');
    if (v > 0xFFFFFFFFu)
      v = 0xFFFFFFFFu;
  }
  *out = (u32)v;
  return 1;
}

/* Redirect tombstones point at live elements (a snapshot invariant the reader
 * enforces), so following is a single hop. */
static u32 follow_redirect(const Hypergraph *g, u32 elem) {
  if (g->elements[elem].redirect != NONE_U32)
    return g->elements[elem].redirect;
  return elem;
}

/* Homonym choice over the by_norm chain for an already-normalized name:
 * prefer a candidate whose kind matches want_kind (when given), then highest
 * last_seen, then highest id (spec §8; the id tie-break is ours to pin).
 * Returns NONE_U32 when no live element carries the name. out_count (when
 * non-NULL) receives the number of live candidates — >1 marks a homonym
 * pick, which write positions report and precise positions reject. Two
 * name-links can point at one element (a name and an alias normalizing the
 * same way), so candidates are counted once per element. */
static u32 resolve_tier1_n(const Hypergraph *g, const char *norm, u32 nlen,
                           u32 want_kind, u32 *out_count) {
  u32 head = smap_get(&g->by_norm, &g->norms, norm, nlen);
  u32 link = head;
  u32 best = NONE_U32;
  u32 count = 0;
  int best_kind = 0;
  while (link != NONE_U32) {
    u32 e = g->name_links[link].elem;
    u32 here = link;
    link = g->name_links[link].next;
    if (g->elements[e].redirect != NONE_U32)
      continue;
    {
      /* one element can hold two links here (a name and an alias that
       * normalize identically); count each element once */
      u32 l2 = head;
      int dup = 0;
      while (l2 != here) {
        if (g->name_links[l2].elem == e) {
          dup = 1;
          break;
        }
        l2 = g->name_links[l2].next;
      }
      if (dup)
        continue;
    }
    {
      int kind_match = want_kind != NONE_U32 && g->elem_kind[e] == want_kind;
      const Stats *st = &g->elements[e].stats;
      count++;
      if (best == NONE_U32) {
        best = e;
        best_kind = kind_match;
        continue;
      }
      if (kind_match != best_kind) {
        if (kind_match) {
          best = e;
          best_kind = 1;
        }
        continue;
      }
      if (st->last_seen != g->elements[best].stats.last_seen) {
        if (st->last_seen > g->elements[best].stats.last_seen)
          best = e;
        continue;
      }
      if (e > best)
        best = e;
    }
  }
  if (out_count)
    *out_count = count;
  return best;
}

static u32 resolve_tier1(const Hypergraph *g, const char *norm, u32 nlen,
                         u32 want_kind) {
  return resolve_tier1_n(g, norm, nlen, want_kind, NULL);
}

/* Reconcile the extended vocabulary (EXT_NAMES) into this store: adopt an
 * element already carrying the name -- a pre-upgrade store may hold one as an
 * ordinary predicate, or a fresh store just minted it -- else mint it, recording
 * the resolved id in g->wk_ext. Idempotent, so it is safe to run on every load;
 * an older store gains the vocabulary (appended past its existing ids) the first
 * time it is opened by this build. Shared by the init seed and the snapshot
 * reader so both paths converge on the same vocabulary. */
static void seed_ext_vocab(Hypergraph *g) {
  u32 i;
  for (i = 0; i < EXT_COUNT; i++) {
    u32 len = (u32)strlen(EXT_NAMES[i]);
    u32 nlen = normalize_into_scratch(EXT_NAMES[i], len);
    u32 id = resolve_tier1(g, g_norm_buf, nlen, NONE_U32);
    if (id == NONE_U32)
      id = mint_element(g, EXT_NAMES[i], len, 1.0, 0.0, 0);
    g->wk_ext[i] = id;
  }
}

/* The SELF element: the agent whose memory this store is.
 *
 * A store is written by exactly one class of author -- the model that reads it
 * back -- so first person has a single referent here, and `me` in a fact means
 * the writer in every session, forever. That is what makes a deictic word safe
 * as a canonical name: there is no second speaker to confuse it with. (Human
 * first person does occur in the store, but only INSIDE quoted source strings,
 * which are opaque text, never references.)
 *
 * It is vocabulary, not content -- the deictic anchor, the same category as
 * `subject` and `source` -- so it seeds at salience 0 like the rest: the hub
 * itself never surfaces in the orientation packet or gets embedded, while
 * facts ABOUT it do. Seeded resolve-or-mint at init and on every load, exactly
 * as the extended vocabulary is, so a store predating this build gains it with
 * no migration. Protected from rename/merge because every claim binding the
 * agent resolves through it.
 *
 * The aliases are not for disambiguation -- there is nothing to disambiguate --
 * but so a session reaching for a different surface form lands on this node
 * instead of minting a second self. "Claude" is deliberately NOT among them:
 * a store may need to talk about the model as a subject in its own right. */
static const char SELF_NAME[] = "me";
static const char *const SELF_ALIASES[] = {"the assistant", "the agent",
                                           "myself"};
static const char SELF_SUMMARY[] =
    "The agent whose memory this store is: the model that reads and writes it, "
    "across every session. First person in a fact resolves here.";

static void seed_self(Hypergraph *g) {
  u32 len = (u32)strlen(SELF_NAME);
  u32 nlen = normalize_into_scratch(SELF_NAME, len);
  u32 id = resolve_tier1(g, g_norm_buf, nlen, NONE_U32);
  if (id == NONE_U32) {
    u32 a;
    id = mint_element(g, SELF_NAME, len, 1.0, 0.0, 0);
    g->elements[id].summary =
        graph_intern(g, SELF_SUMMARY, (u32)strlen(SELF_SUMMARY));
    /* aliases attach at MINT only: re-running on an adopted element would
     * re-push names every load */
    for (a = 0; a < sizeof SELF_ALIASES / sizeof SELF_ALIASES[0]; a++) {
      u32 alen = (u32)strlen(SELF_ALIASES[a]);
      u32vec_push(&g->elements[id].names,
                  graph_intern(g, SELF_ALIASES[a], alen));
      index_norm_name(g, id, SELF_ALIASES[a], alen);
    }
  }
  g->wk_self = id;
}

/* Is `id` a protected vocabulary element (legacy core 0..WK_ELEMENT_COUNT-1,
 * an extended-vocab name, or the self anchor)? Renaming or merging one away
 * would break resolution of every claim that uses it. */
static int is_core_vocab(const Hypergraph *g, u32 id) {
  u32 i;
  if (id < WK_ELEMENT_COUNT)
    return 1;
  if (g->wk_self == id)
    return 1;
  for (i = 0; i < EXT_COUNT; i++)
    if (g->wk_ext[i] == id)
      return 1;
  return 0;
}

/* Element id backing one MODF_* modal bit: intervened is legacy core, the rest
 * are extended vocab resolved through this store's g->wk_ext. */
static u32 modal_elem_id(const Hypergraph *g, u32 bit) {
  switch (bit) {
  case MODF_INTERVENED:
    return WK_INTERVENED;
  case MODF_NEGATED:
    return g->wk_ext[EXT_NEGATED];
  case MODF_UNCERTAIN:
    return g->wk_ext[EXT_UNCERTAIN];
  case MODF_NON_ACTUAL:
    return g->wk_ext[EXT_NON_ACTUAL];
  case MODF_GENERAL:
    return g->wk_ext[EXT_GENERAL];
  }
  return NONE_U32;
}

static const char *elem_name(const Hypergraph *g, u32 id) {
  return str_ptr(&g->strs, g->elements[id].names.v[0]);
}
static u32 elem_name_l(const Hypergraph *g, u32 id) {
  return str_len(&g->strs, g->elements[id].names.v[0]);
}

/* Bounded JSON string escaping into dst; returns bytes written. Truncates by
 * whole units — an escape or a full UTF-8 sequence — never mid-unit, so the
 * output is always valid JSON string content. Malformed bytes are dropped. */
static u32 json_escape_buf(char *dst, u32 cap, const char *s, u32 len) {
  u32 i = 0, w = 0;
  while (i < len) {
    unsigned char c = (unsigned char)s[i];
    char unit[8];
    u32 ulen;
    if (c == '"' || c == '\\') {
      unit[0] = '\\';
      unit[1] = (char)c;
      ulen = 2;
      i++;
    } else if (c == '\n') {
      memcpy(unit, "\\n", 2);
      ulen = 2;
      i++;
    } else if (c == '\r') {
      memcpy(unit, "\\r", 2);
      ulen = 2;
      i++;
    } else if (c == '\t') {
      memcpy(unit, "\\t", 2);
      ulen = 2;
      i++;
    } else if (c < 0x20) {
      ulen = (u32)snprintf(unit, sizeof unit, "\\u%04x", c);
      i++;
    } else if (c < 0x80) {
      unit[0] = (char)c;
      ulen = 1;
      i++;
    } else {
      ulen = utf8_seq_len((const unsigned char *)s + i, len - i);
      if (ulen == 0) {
        i++;
        continue;
      } /* truncation artifact: drop */
      memcpy(unit, s + i, ulen);
      i += ulen;
    }
    if (w + ulen > cap)
      break;
    memcpy(dst + w, unit, ulen);
    w += ulen;
  }
  return w;
}

/* Live elements carrying a normalized name, counted once each, id ascending. */
static void collect_homonyms(const Hypergraph *g, const char *norm, u32 nlen,
                             U32Vec *out) {
  u32 link = smap_get(&g->by_norm, &g->norms, norm, nlen);
  u32 i;
  out->count = 0;
  while (link != NONE_U32) {
    u32 e = g->name_links[link].elem;
    link = g->name_links[link].next;
    if (g->elements[e].redirect != NONE_U32)
      continue;
    for (i = 0; i < out->count; i++)
      if (out->v[i] == e)
        break;
    if (i == out->count)
      u32vec_push(out, e);
  }
  u32vec_sort(out, u32_cmp);
}

/* Stage candidate objects ({ref, name, kind?, score} — plan §3.15) into the
 * error envelope. Tier-1 candidates all score 1; entries that would overflow
 * the buffer are dropped from the tail. */
static void stage_error_candidates(const Hypergraph *g, const u32 *elems,
                                   u32 n) {
  u32 i, w = 0;
  char *b = g_err_candidates;
  const u32 cap = sizeof g_err_candidates - 2;
  b[w++] = '[';
  for (i = 0; i < n; i++) {
    char entry[512];
    u32 el = 0, kind = g->elem_kind[elems[i]];
    el +=
        (u32)snprintf(entry + el, sizeof entry - el,
                      "%s{\"ref\":\"#%u\",\"name\":\"", i ? "," : "", elems[i]);
    el += json_escape_buf(entry + el, (u32)(sizeof entry - el - 64),
                          elem_name(g, elems[i]), elem_name_l(g, elems[i]));
    el += (u32)snprintf(entry + el, sizeof entry - el, "\"");
    if (kind != NONE_U32) {
      el += (u32)snprintf(entry + el, sizeof entry - el, ",\"kind\":\"");
      el += json_escape_buf(entry + el, (u32)(sizeof entry - el - 16),
                            elem_name(g, kind), elem_name_l(g, kind));
      el += (u32)snprintf(entry + el, sizeof entry - el, "\"");
    }
    el += (u32)snprintf(entry + el, sizeof entry - el, ",\"score\":1}");
    if (w + el > cap)
      break;
    memcpy(b + w, entry, el);
    w += el;
  }
  b[w++] = ']';
  b[w] = 0;
}

/* Precise-position resolution (spec §8: retract and merge refs): tier 1 or
 * explicit id only — an action that flips status or folds elements never
 * executes off a similarity guess. A tier-1 homonym is ambiguous_ref with
 * candidates; no match is unknown_ref. Redirect tombstones keep working. */
static u32 resolve_precise_elem(const Hypergraph *g, const char *buf, Span s,
                                const char *path) {
  u32 id, nlen, count;
  static U32Vec homonyms;
  if (span_parse_elem_id(buf, s, &id)) {
    if (id >= g->element_count)
      fail(ERR_UNKNOWN_REF, path, "unknown element #%u", id);
    return follow_redirect(g, id);
  }
  if (span_is_rel_ref(buf, s))
    fail(ERR_PARSE, path, "expected an element ref, got a relation ref");
  nlen = normalize_into_scratch(buf + s.off, s.len);
  if (nlen == 0)
    fail(ERR_PARSE, path, "name is empty after normalization");
  id = resolve_tier1_n(g, g_norm_buf, nlen, NONE_U32, &count);
  if (id == NONE_U32)
    fail(ERR_UNKNOWN_REF, path, "no element named \"%.*s\"",
         (int)(s.len > 80 ? 80 : s.len), buf + s.off);
  if (count > 1) {
    collect_homonyms(g, g_norm_buf, nlen, &homonyms);
    stage_error_candidates(g, homonyms.v, homonyms.count);
    fail(ERR_AMBIGUOUS_REF, path, "\"%.*s\" names %u elements",
         (int)(s.len > 80 ? 80 : s.len), buf + s.off, count);
  }
  return id;
}

/* ---- S5 tier 2: trigram similarity over names, aliases, and summaries ----
 * Read positions (focus): score = max trigram *containment* of the query in
 * each of the element's names, aliases, and summary; >= 0.6 joins the walk
 * (reported via "lexical"), below that candidates >= 0.3 report on a miss.
 * Write positions (near_matches): score = max symmetric *Jaccard* against
 * names and aliases only — summaries would false-positive every mint whose
 * name appears inside an existing summary (the spec's own worked example
 * demands near_matches [] while its focus resolves through a summary). */

enum { TIER2_CAND_CAP = 8 }; /* unresolved-ref candidate list bound */
/* Auto-resolve a read focus only on a slam-dunk: a unique, strong lexical
 * match. Weaker or ambiguous scans report resolved:false and let the caller
 * pick from the candidate list, rather than guessing (and guessing wrong —
 * containment saturates on shared summary words, so a moderate score is not
 * confidence). */
static const double TIER2_AUTO_SCORE = 0.7;   /* min top score to auto-resolve */
static const double TIER2_AUTO_MARGIN = 0.15; /* min lead over the 2nd candidate */
/* An ambient recall (the passive hook sweep, observe:true) over a lived-in store
 * competes with its own in-band process records — testimonies, tasks, open
 * questions, doc pointers — whose names share vocabulary with real content, so
 * they crowd the candidate list ahead of the domain graph. On an ambient miss
 * only, scale these bookkeeping kinds' candidate scores down so domain content
 * leads; deliberate recalls (observe:false) are untouched, and nothing is
 * dropped — the frame still lists them, just lower. */
static const double TIER2_AMBIENT_BOOKKEEPING_FACTOR = 0.5;
/* Ambient abstention: a passive sweep whose best lexical match is below this
 * shares no real vocabulary with the store — almost certainly an off-domain
 * prompt (an art critique, a stack trace) whose semantic ranking is just the
 * top-salience defaults as clutter. Below it, an ambient recall skips the
 * semantic/backfill append and stays quiet; deliberate recalls never abstain. */
static const double TIER2_AMBIENT_ANCHOR = 0.6;
/* Ambient recall with a weak lexical anchor is NOT automatically off-domain: a
 * paraphrase ("verge region music" -> "The Verge") has little trigram overlap but a
 * strong SEMANTIC match. When the lexical anchor is below TIER2_AMBIENT_ANCHOR, fall
 * back to the semantic ranking and surface it only if its best clears this bar;
 * genuine off-domain prompts clear neither and stay quiet. Tuned on the trial store:
 * on-domain best scores >=0.76, off-domain <=0.74 (thin margin -- watch/retune). */
static const double TIER2_AMBIENT_SEMANTIC_MIN = 0.75;
enum { TIER2_LIST_CAP = 100 }; /* miss candidate list bound after salience backfill */
enum { TIER2_SEMANTIC_CAP = 30 }; /* shorter list when semantic: the target ranks high,
                                     so a fraction of the salience roster suffices */

typedef struct {
  u32 elem;
  double score;
} ScoredElem;
typedef struct {
  ScoredElem *v;
  u32 count, cap;
} ScoredVec;

static void scored_push(ScoredVec *s, u32 elem, double score) {
  s->v = (ScoredElem *)xgrow(s->v, s->count + 1, &s->cap, sizeof *s->v);
  s->v[s->count].elem = elem;
  s->v[s->count].score = score;
  s->count++;
}

/* score desc, then id asc (plan §5 candidate ordering) */
static int scored_cmp(const void *pa, const void *pb) {
  const ScoredElem *a = (const ScoredElem *)pa, *b = (const ScoredElem *)pb;
  if (a->score != b->score)
    return a->score > b->score ? -1 : 1;
  return a->elem < b->elem ? -1 : a->elem > b->elem ? 1 : 0;
}

/* Trigram set of the normalized form of s. Clobbers g_norm_buf. */
static void tier2_trigrams(const char *s, u32 len, U32Vec *out) {
  u32 nlen = normalize_into_scratch(s, len);
  trigram_set_build(g_norm_buf, nlen, out);
}

static U32Vec g_t2_target; /* target-side trigram scratch */

/* Max containment of the query trigrams in one element's lexical corpus. */
static double tier2_read_score(const Hypergraph *g, const U32Vec *q, u32 elem) {
  const Element *e = &g->elements[elem];
  double best = 0.0, s;
  u32 k;
  for (k = 0; k < e->names.count; k++) {
    tier2_trigrams(str_ptr(&g->strs, e->names.v[k]),
                   str_len(&g->strs, e->names.v[k]), &g_t2_target);
    s = trigram_containment(q->v, q->count, g_t2_target.v, g_t2_target.count);
    if (s > best)
      best = s;
  }
  if (e->summary != NONE_U32) {
    tier2_trigrams(str_ptr(&g->strs, e->summary), str_len(&g->strs, e->summary),
                   &g_t2_target);
    s = trigram_containment(q->v, q->count, g_t2_target.v, g_t2_target.count);
    if (s > best)
      best = s;
  }
  return best;
}

/* Max symmetric Jaccard against one element's names and aliases. */
static double tier2_near_score(const Hypergraph *g, const U32Vec *q, u32 elem) {
  const Element *e = &g->elements[elem];
  double best = 0.0, s;
  u32 k;
  for (k = 0; k < e->names.count; k++) {
    tier2_trigrams(str_ptr(&g->strs, e->names.v[k]),
                   str_len(&g->strs, e->names.v[k]), &g_t2_target);
    s = trigram_jaccard(q->v, q->count, g_t2_target.v, g_t2_target.count);
    if (s > best)
      best = s;
  }
  return best;
}

/* All live elements scoring >= min_score for a read-position query, sorted
 * score desc then id asc. The query bytes are raw (pre-normalization). */
static void tier2_scan_read(const Hypergraph *g, const char *q, u32 qlen,
                            double min_score, ScoredVec *out) {
  static U32Vec qtri;
  u32 e;
  out->count = 0;
  tier2_trigrams(q, qlen, &qtri);
  for (e = 0; e < g->element_count; e++) {
    double s;
    if (g->elements[e].redirect != NONE_U32)
      continue;
    s = tier2_read_score(g, &qtri, e);
    if (s >= min_score)
      scored_push(out, e, s);
  }
  if (out->count > 1)
    qsort(out->v, out->count, sizeof *out->v, scored_cmp);
}

/* Broaden a miss's candidate list: after the lexical near-matches, append the
 * store's live elements by salience desc (id asc on ties), skipping any already
 * listed, so the caller has the most significant memories to scan for a
 * semantic pick. Backfill entries carry score 0 (they are not lexical matches).
 * This is the list-pick path that stands in for embeddings while the store is
 * small enough to enumerate. */
static void tier2_backfill(const Hypergraph *g, ScoredVec *cands, u32 cand_start,
                           u32 cap) {
  static ScoredVec roster;
  u32 e, k;
  roster.count = 0;
  for (e = 0; e < g->element_count; e++) {
    int present = 0;
    if (g->elements[e].redirect != NONE_U32) /* merged-away tombstone */
      continue;
    if (g->elements[e].stats.salience <= 0.0) /* vocab/meta: not worth listing */
      continue;
    for (k = cand_start; k < cands->count; k++)
      if (cands->v[k].elem == e) {
        present = 1;
        break;
      }
    if (!present)
      scored_push(&roster, e, g->elements[e].stats.salience);
  }
  if (roster.count > 1)
    qsort(roster.v, roster.count, sizeof *roster.v, scored_cmp);
  for (e = 0; e < roster.count && (cands->count - cand_start) < cap; e++)
    scored_push(cands, roster.v[e].elem, 0.0);
}

/* Build the embeddable-element list (live, salient, named): each element's
 * "name. summary" text + its id, the same filter tier2_backfill uses. Caller
 * releases with free_embed_list. Returns count; idv,txv set NULL on alloc fail. */
static u32 build_embed_list(const Hypergraph *g, u32 **out_idv, char ***out_txv) {
  u32 e, n = 0;
  u32 *idv = malloc((size_t)g->element_count * sizeof *idv);
  char **txv = malloc((size_t)g->element_count * sizeof *txv);
  if (!idv || !txv) { free(idv); free(txv); *out_idv = NULL; *out_txv = NULL; return 0; }
  for (e = 0; e < g->element_count; e++) {
    const Element *el = &g->elements[e];
    const char *nm, *sm;
    u32 nl, sl, p;
    char *t;
    if (el->redirect != NONE_U32)      /* merged-away tombstone */
      continue;
    if (el->stats.salience <= 0.0)     /* vocab/meta: not worth embedding */
      continue;
    if (el->names.count == 0)
      continue;
    nm = str_ptr(&g->strs, el->names.v[0]);
    nl = str_len(&g->strs, el->names.v[0]);
    sm = el->summary != NONE_U32 ? str_ptr(&g->strs, el->summary) : NULL;
    sl = sm ? str_len(&g->strs, el->summary) : 0;
    t = malloc((size_t)nl + sl + 3);
    if (!t)
      continue;
    memcpy(t, nm, nl);
    p = nl;
    if (sl) { t[p++] = '.'; t[p++] = ' '; memcpy(t + p, sm, sl); p += sl; }
    t[p] = 0;
    idv[n] = e;
    txv[n] = t;
    n++;
  }
  *out_idv = idv;
  *out_txv = txv;
  return n;
}

static void free_embed_list(u32 *idv, char **txv, u32 n) {
  u32 i;
  if (txv)
    for (i = 0; i < n; i++)
      free(txv[i]);
  free(idv);
  free(txv);
}

/* Semantic miss-fallback: cosine the query against element embeddings (name +
 * summary) and append the nearest as candidates, score = cosine. Returns 1 when
 * embeddings ran, 0 when unavailable (model disabled/absent) so the caller uses
 * the salience backfill instead — the semantically-ranked list is the sharp
 * form of the blunt salience roster; both are "a list for the caller to pick". */
static int tier2_semantic(const Hypergraph *g, ScoredVec *cands, u32 cand_start,
                          u32 cap, const char *query, u32 qlen) {
  u32 *idv, *rid, n, i;
  char **txv;
  float *rsc;
  int r = -1;
  u32 scap = cap < TIER2_SEMANTIC_CAP ? cap : TIER2_SEMANTIC_CAP;
  if (!embed_available()) /* skip building the element list when disabled */
    return 0;
  n = build_embed_list(g, &idv, &txv);
  rid = malloc((size_t)scap * sizeof *rid);
  rsc = malloc((size_t)scap * sizeof *rsc);
  if (idv && txv && rid && rsc)
    r = embed_rank_elements(idv, (const char *const *)txv, (int)n, query,
                            (int)qlen, rid, rsc, (int)scap);
  free_embed_list(idv, txv, n);
  if (r < 0) { free(rid); free(rsc); return 0; }
  for (i = 0; i < (u32)r && (cands->count - cand_start) < scap; i++) {
    u32 k;
    int present = 0;
    for (k = cand_start; k < cands->count; k++)
      if (cands->v[k].elem == rid[i]) { present = 1; break; }
    if (!present)
      scored_push(cands, rid[i], rsc[i]);
  }
  free(rid);
  free(rsc);
  return 1;
}

/* Eager embed: after a save, ensure every embeddable element has a current
 * vector in the sidecar (only new/changed ones are computed), so the cost
 * amortizes across saves and the first recall miss never rebuilds the cache. */
static void tier2_sync(const Hypergraph *g) {
  u32 *idv, n;
  char **txv;
  if (!embed_enabled()) /* cheap gate: no 23 MB load when nothing may need it */
    return;
  n = build_embed_list(g, &idv, &txv);
  if (idv && txv)
    embed_sync_elements(idv, (const char *const *)txv, (int)n);
  free_embed_list(idv, txv, n);
}

/* Best pre-existing near match for a freshly minted write-position name:
 * highest Jaccard >= 0.6, ties lowest id. Returns NONE_U32 when none. */
static u32 tier2_best_near(const Hypergraph *g, const char *q, u32 qlen,
                           double *out_score) {
  static U32Vec qtri;
  u32 e, best = NONE_U32;
  double best_s = 0.0;
  tier2_trigrams(q, qlen, &qtri);
  for (e = 0; e < g->element_count; e++) {
    double s;
    if (g->elements[e].redirect != NONE_U32)
      continue;
    s = tier2_near_score(g, &qtri, e);
    if (s >= 0.6 && s > best_s) {
      best_s = s;
      best = e;
    }
  }
  *out_score = best_s;
  return best;
}

/* True when an element is ambient-recall noise rather than domain content:
 * either it carries no instance_of kind — a relation predicate (subject, to,
 * resolves…) or a prose-object mint — or its kind is explicit process
 * bookkeeping (event, task, question, pointer). Both flood a lived-in store's
 * candidate lists ahead of the game graph. Kind is the first live instance_of,
 * cached in elem_kind; the kind element's name is compared by length (the
 * string arena is not NUL-terminated). decision and constraint are deliberately
 * content — a design decision or a constraint is real. */
static int elem_is_ambient_noise(const Hypergraph *g, u32 e) {
  u32 k = g->elem_kind[e];
  const char *n;
  u32 nl;
  if (k == NONE_U32)                     /* predicate label or prose-object mint */
    return 1;
  if (g->elements[k].names.count == 0)   /* kinded but unnamed: treat as content */
    return 0;
  n = str_ptr(&g->strs, g->elements[k].names.v[0]);
  nl = str_len(&g->strs, g->elements[k].names.v[0]);
  return (nl == 5 && memcmp(n, "event", 5) == 0) ||
         (nl == 4 && memcmp(n, "task", 4) == 0) ||
         (nl == 8 && memcmp(n, "question", 8) == 0) ||
         (nl == 7 && memcmp(n, "pointer", 7) == 0);
}

/* Best NAME-only trigram containment of the query across live elements — the
 * ambient-abstention anchor. Names are content-specific; a match against a
 * summary's prose (which shares common words with any prompt) is not a real
 * anchor, so abstention uses this stricter signal rather than the scan score
 * (which also counts summary containment). */
static double tier2_name_anchor(const Hypergraph *g, const char *q, u32 qlen) {
  static U32Vec qtri;
  double best = 0.0;
  u32 e, k;
  tier2_trigrams(q, qlen, &qtri);
  for (e = 0; e < g->element_count; e++) {
    const Element *el = &g->elements[e];
    if (el->redirect != NONE_U32)
      continue;
    for (k = 0; k < el->names.count; k++) {
      double s;
      tier2_trigrams(str_ptr(&g->strs, el->names.v[k]),
                     str_len(&g->strs, el->names.v[k]), &g_t2_target);
      /* name-coverage: what fraction of the NAME's trigrams the prompt contains
       * — i.e. does the prompt mention (most of) this element's name. The other
       * direction (query-coverage) would abstain a long prompt that names an
       * entity in passing. */
      s = trigram_containment(g_t2_target.v, g_t2_target.count, qtri.v,
                              qtri.count);
      if (s > best)
        best = s;
    }
  }
  return best;
}

/* The tier-2 focus-miss policy, shared by the save-path focus walk and
 * tick_recall: auto-resolve only a slam-dunk (a unique, strong lexical match —
 * containment saturates on shared summary words, so a moderate score is not
 * confidence) and return the element with *out_score set. Anything weaker or
 * ambiguous returns NONE_U32 after appending the caller's candidate list to
 * cands: lexical near-matches down to 0.3, then the semantic ranking or, when
 * embeddings are unavailable, the salience backfill. On an ambient recall
 * (observe:true) bookkeeping kinds are demoted so domain content leads. */
static u32 tier2_focus_miss(const Hypergraph *g, const char *q, u32 qlen,
                            ScoredVec *cands, u32 *out_cand_count,
                            double *out_score, int ambient) {
  static ScoredVec scan;
  u32 c, start = cands->count;
  int slam;
  tier2_scan_read(g, q, qlen, 0.3, &scan);
  slam = scan.count > 0 && scan.v[0].score >= TIER2_AUTO_SCORE &&
         (scan.count == 1 ||
          scan.v[0].score - scan.v[1].score >= TIER2_AUTO_MARGIN);
  if (slam) {
    *out_score = scan.v[0].score;
    return scan.v[0].elem;
  }
  /* A deliberate recall, or an ambient sweep with a real lexical anchor, gets the
   * full list: lexical near-matches, then the semantic ranking (or salience
   * backfill when embeddings are off). */
  if (!ambient || tier2_name_anchor(g, q, qlen) >= TIER2_AMBIENT_ANCHOR) {
    for (c = 0; c < scan.count && c < TIER2_CAND_CAP; c++)
      scored_push(cands, scan.v[c].elem, scan.v[c].score);
    if (!tier2_semantic(g, cands, start, TIER2_LIST_CAP, q, qlen))
      tier2_backfill(g, cands, start, TIER2_LIST_CAP);
  } else if (tier2_semantic(g, cands, start, TIER2_LIST_CAP, q, qlen)) {
    /* Ambient sweep, weak lexical anchor, embeddings available: don't abstain
     * blindly — surface the semantic ranking only if its best clears the bar (a
     * strong paraphrase match); otherwise stay quiet (off-domain clutter). The
     * lexical scan is skipped here, so its low-overlap noise never leaks in. */
    double best = 0.0;
    for (c = start; c < cands->count; c++)
      if (cands->v[c].score > best)
        best = cands->v[c].score;
    if (best < TIER2_AMBIENT_SEMANTIC_MIN)
      cands->count = start; /* semantic is clutter too — abstain */
  }
  /* Ambient miss: demote bookkeeping kinds, then re-sort the whole pool
   * (lexical + semantic together) by the adjusted score. Reorder only — no
   * candidate is dropped, so presence-based recall is unchanged and deliberate
   * recalls skip this path entirely. */
  if (ambient) {
    for (c = start; c < cands->count; c++)
      if (elem_is_ambient_noise(g, cands->v[c].elem))
        cands->v[c].score *= TIER2_AMBIENT_BOOKKEEPING_FACTOR;
    if (cands->count - start > 1)
      qsort(cands->v + start, cands->count - start, sizeof *cands->v, scored_cmp);
  }
  *out_cand_count = cands->count - start;
  return NONE_U32;
}

/* ============================= S7 — dynamics ==============================
 *
 * The "physics" of memory, in plain words: touch something and it warms up
 * (activation rises); ignore it and its availability cools (activation
 * decays); do this over spread-out sessions and it becomes sturdier than a
 * one-off cram (stability); mark something important and it resists cooling
 * (salience). The one hard rule: only AVAILABILITY decays — TRUTH never does.
 * Decay touches stats.activation and nothing else; confidence, status, and
 * supersession history are never eroded by neglect. A fact you haven't looked
 * at in months is still true, just less "warm", so it ranks lower.
 *
 * This is what makes orientation mode surface "what matters now" and what
 * makes a stale relation fall below a fresh one in `related`. It is defined
 * BEFORE S6 in the file because apply (the write path) calls into it at the
 * end of every tick.
 *
 * Live dynamics (spec §3.1, plan §2 S7): activation bump on touch, spaced-vs-
 * massed stability, utility-modulated activation decay with exception
 * protection, importance effect-topology, arousal salience bump. Operators are
 * v2/v1 ports per pin 6; everything runs on doubles under strict IEEE, so a
 * replay of the same (store, payload, clock) stream is bit-identical. */

static double clamp_unit(double x) { return x < 0.0 ? 0.0 : x > 1.0 ? 1.0 : x; }

/* Bounded Hebbian operators (v2:hebbian.rs, Oja-rule-derived): bump moves x
 * toward 1 and decay toward 0, both closed over [0, 1]. */
static double hebb_bump(double x, double rate) {
  return clamp_unit(x + rate * (1.0 - x));
}
static double hebb_decay(double x, double rate) {
  return clamp_unit(x * (1.0 - rate));
}

/* Stability soft cap (v1:neocortex.rs soft_cap_stability): linear to the knee,
 * tanh-compressed above it, asymptote at cap. v1's constants were knee 10 /
 * max 20 / scale 10 — expressed here as cap/2 so policy.stability_cap (20)
 * reproduces them exactly. */
static double soft_cap_stability(double raw, double cap) {
  double knee = cap * 0.5;
  if (raw <= knee)
    return raw;
  return knee + (cap - knee) * tanh((raw - knee) / knee);
}

/* Importance effect-topology constants (v1:mod.rs CPEB tagging): a caller
 * salience above the threshold adds boost x salience to the stability of
 * relations minted the same tick that touch the salience carrier. */
static const double DYN_CPEB_THRESHOLD = 0.3;
static const double DYN_CPEB_BOOST = 1.5;

/* One dynamics touch, at most once per object per tick (the last_seen == tick
 * guard): bump activation, then the spaced-vs-massed stability update (pin 6:
 * x1.3 when the touch interval grew, x1.05 when massed, tanh-capped).
 *
 * The interval rule works entirely from frozen snapshot fields — the v1
 * original stored per-object interval EMAs, but the M1-frozen stat block has
 * no such slot, so the prior-interval mean is derived instead: prior touches
 * span (last_seen - created_at) ticks over access_count intervals (mint plus
 * one last_seen update per counted access). A touch whose interval exceeds
 * that mean is spaced; the first re-touch (access_count 0) has no prior
 * interval and only initializes the record, exactly like v1's first
 * reinforcement. Callers update access_count/last_seen after this returns. */
static int dyn_touch(Stats *st, u32 created_at, u32 tick, const Policy *po) {
  if (st->last_seen == tick)
    return 0;
  st->activation = hebb_bump(st->activation, po->hebbian_rate);
  if (st->access_count >= 1 && tick > st->last_seen &&
      st->last_seen > created_at) {
    double cur = (double)(tick - st->last_seen);
    double mean =
        (double)(st->last_seen - created_at) / (double)st->access_count;
    double factor = cur > mean ? po->stability_spaced : po->stability_massed;
    st->stability =
        soft_cap_stability(st->stability * factor, po->stability_cap);
  }
  return 1;
}

/* Arousal salience bump (spec §10: intent arousal scales the salience bump):
 * a touched relation's salience climbs at hebbian_rate scaled by the
 * payload's arousal. */
static void dyn_arousal_salience(Stats *st, const Policy *po, double arousal) {
  st->salience =
      hebb_bump(st->salience, clamp_unit(po->hebbian_rate * arousal));
}

static int rel_event_shaped(const Hypergraph *g, u32 rid) {
  const Relation *r = &g->relations[rid];
  u32 a;
  for (a = 0; a < r->attr_count; a++)
    if (r->attrs[a].name == WK_FROM || r->attrs[a].name == WK_TO)
      return 1;
  return 0;
}

static int elem_name_has_digit(const Hypergraph *g, u32 e) {
  u32 sid = g->elements[e].names.v[0];
  const char *b = str_ptr(&g->strs, sid);
  u32 n = str_len(&g->strs, sid), i;
  for (i = 0; i < n; i++)
    if (b[i] >= '0' && b[i] <= '9')
      return 1;
  return 0;
}

/* Exception protection (spec §3.1, from v1's sleep down-selection): relations
 * carrying quantity/date-shaped values or supersession events never decay.
 * The byte-level rule: protected iff the relation is event-shaped (a from/to
 * slot — every change event and supersession-triggering fact) or any element-
 * valued slot's canonical name contains an ASCII digit 0-9 (v1's catch-all
 * for quantities, dates, versions, and refs). */
static int rel_exception_protected(const Hypergraph *g, u32 rid) {
  const Relation *r = &g->relations[rid];
  u32 a;
  if (rel_event_shaped(g, rid))
    return 1;
  for (a = 0; a < r->attr_count; a++)
    if (r->attrs[a].value.tag == TERM_ELEM &&
        elem_name_has_digit(g, r->attrs[a].value.id))
      return 1;
  return 0;
}

/* The effective per-object decay rate (v2:decay.rs shape plus the §3.1
 * modifiers, composed in this order):
 *   1. utility u = focus_success_count + support_count + salience, soft-capped
 *      to [0,1) via u/(u+5); rate = decay_rate * (1 - that) — high-utility
 *      objects keep a small positive floor, low-utility ones decay at full
 *      rate;
 *   2. salience divides: /(1 + salience) — v1's half-rate for high valence;
 *   3. stability divides — spaced-access objects outlast massed ones. */
static double decay_effective_rate(const Stats *st, const Policy *po) {
  double u, rate;
  if (po->decay_rate <= 0.0)
    return 0.0;
  u = (double)st->focus_success_count + (double)st->support_count +
      st->salience;
  rate = po->decay_rate * (1.0 - (u <= 0.0 ? 0.0 : u / (u + 5.0)));
  rate /= 1.0 + st->salience;
  if (st->stability > 1.0)
    rate /= st->stability;
  return clamp_unit(rate);
}

/* Focus-radius decay (v2:decay.rs focus_radius_decay, ported): bounded BFS
 * outward from the tick's reinforced relations, decaying only stats.activation
 * — availability fades, truth never does (confidence, status, supersession
 * untouched). Seed elements (the slots of the reinforced relations, depth 0)
 * are never decayed; elements first reached at depth >= 1 decay, and every
 * relation reachable from an element within the radius decays once, unless it
 * was itself touched this tick (last_seen == tick) or exception-protected. */
static void dyn_decay_walk(Hypergraph *g, const U32Vec *reinforced, u32 tick,
                           const Policy *po) {
  static u8 *e_seen;
  static u32 e_seen_cap;
  static u8 *r_done;
  static u32 r_done_cap;
  static U32Vec queue, qdepth;
  u32 i, head = 0;
  if (po->focus_decay_radius == 0 || po->decay_rate <= 0.0)
    return;
  e_seen = (u8 *)xgrow(e_seen, g->element_count ? g->element_count : 1,
                       &e_seen_cap, 1);
  r_done = (u8 *)xgrow(r_done, g->relation_count ? g->relation_count : 1,
                       &r_done_cap, 1);
  memset(e_seen, 0, g->element_count);
  memset(r_done, 0, g->relation_count);
  queue.count = qdepth.count = 0;
  for (i = 0; i < reinforced->count; i++) {
    const Relation *r = &g->relations[reinforced->v[i]];
    u32 a;
    for (a = 0; a < r->attr_count; a++) {
      u32 e = r->attrs[a].value.id;
      if (r->attrs[a].value.tag != TERM_ELEM || e_seen[e])
        continue;
      e_seen[e] = 1;
      u32vec_push(&queue, e);
      u32vec_push(&qdepth, 0);
    }
  }
  while (head < queue.count) {
    u32 elem = queue.v[head], depth = qdepth.v[head], link;
    head++;
    if (depth >= po->focus_decay_radius)
      continue;
    link = g->rels_by_elem[elem];
    while (link != NONE_U32) {
      u32 rid = g->rel_links[link].rel;
      link = g->rel_links[link].next;
      if (!r_done[rid]) {
        Relation *r = &g->relations[rid];
        r_done[rid] = 1;
        if (r->stats.last_seen != tick && !rel_exception_protected(g, rid))
          r->stats.activation = hebb_decay(r->stats.activation,
                                           decay_effective_rate(&r->stats, po));
      }
      {
        const Relation *r = &g->relations[rid];
        u32 a;
        for (a = 0; a < r->attr_count; a++) {
          u32 e = r->attrs[a].value.id;
          if (r->attrs[a].value.tag != TERM_ELEM || e == elem || e_seen[e])
            continue;
          e_seen[e] = 1;
          if (g->elements[e].stats.last_seen != tick) {
            Element *el = &g->elements[e];
            el->stats.activation = hebb_decay(
                el->stats.activation, decay_effective_rate(&el->stats, po));
          }
          u32vec_push(&queue, e);
          u32vec_push(&qdepth, depth + 1);
        }
      }
    }
  }
}

/* Focus reinforcement, shared by the save-path focus ops and tick_recall
 * (spec §6: recall marks paths for future ranking): the focus element and its
 * live one-hop relations each take a full dynamics touch plus their
 * focus_success_count credit; the touched relations join the decay-walk seed
 * set. The caller has already deduped focus elements for this tick. */
static void dyn_reinforce_focus(Hypergraph *g, u32 e, u32 tick, double arousal,
                                U32Vec *reinforced) {
  Element *el = &g->elements[e];
  u32 link;
  /* access_count gates the interval math (see dyn_touch), so it counts
   * distinct touched ticks: bump it only when the touch fired. */
  if (dyn_touch(&el->stats, el->created_at, tick, &g->policy))
    el->stats.access_count++;
  el->stats.focus_success_count++;
  el->stats.last_seen = tick;
  link = g->rels_by_elem[e];
  while (link != NONE_U32) {
    u32 rid = g->rel_links[link].rel;
    Relation *r = &g->relations[rid];
    link = g->rel_links[link].next;
    if (r->status >= ST_SUPERSEDED)
      continue;
    if (dyn_touch(&r->stats, r->created_at, tick, &g->policy)) {
      dyn_arousal_salience(&r->stats, &g->policy, arousal);
      r->stats.access_count++;
    }
    r->stats.focus_success_count++;
    r->stats.last_seen = tick;
    u32vec_push_unique(reinforced, rid);
  }
}

/* ===================== S6 — write path (M1 subset) =======================
 *
 * THE HEART OF THE SAVE PATH. Every `save` runs as TWO passes over the graph —
 * this is the plan/apply architecture (v3_c_plan.md §4), and understanding it
 * is the key to the write side of the file. `recall` has its own read path:
 * it resolves focus refs, then reinforces and decays memory dynamics unless
 * `observe:true`.
 *
 *   PASS 1 — PLAN (plan_submission): validate EVERYTHING, mutate NOTHING.
 *     Walk the whole payload in a fixed order and, for every ref, decide what
 *     WOULD happen: this name resolves to element #42 (reuse), that one is new
 *     (mint), this change supersedes that prior, this retract flips those
 *     relations. All of it is recorded as a list of staged "ops" that point at
 *     PENDS (pending elements, not yet real) and plan-rel indices (pending
 *     relations). Crucially, plan only ever calls find()-style lookups — it
 *     never interns a string into the graph, never bumps a counter, never
 *     flips a status. If any ref is bad, plan calls fail() and the store is
 *     left byte-for-byte as it was, clock included.
 *
 *   PASS 2 — APPLY (apply_plan): execute the staged ops in a fixed order
 *     (templates -> elements -> facts -> changes -> retract -> merge -> metas
 *     -> promotion -> dynamics). This is the ONLY place the graph mutates.
 *     Apply cannot fail except on disk I/O — all the "can this work?" questions
 *     were answered in plan.
 *
 * WHY the two passes matter for a MEMORY store: it makes a submission
 * ALL-OR-NOTHING for free. LLMs retry calls constantly; if a half-applied
 * write left a fact minted but its support_count double-bumped, promotion (the
 * "repetition becomes evidence" mechanism) would rot. Splitting validate from
 * execute means a rejected or interrupted submission changes nothing, so a
 * retry is always safe. It also gives the fuzzer one clean invariant: any
 * input either errors cleanly or applies fully.
 *
 * WHY the intermediate "op" vocabulary (PendElem, PlanRel, FlipOp, CurOv, ...):
 * plan can't use real element/relation ids for things it hasn't minted yet, so
 * it invents PENDS — a pend is "the element this name will resolve to, whether
 * reused or freshly minted." Ops reference pends and plan-rel indices; apply
 * walks the ops and swaps in the real ids as it mints them. The g_pend_real /
 * g_rel_real arrays (near apply_plan) are that pend-index -> real-id map.
 *
 * IDEMPOTENT NO-OPS are staged as ops too, never as errors (spec §9): retract
 * of an already-Retracted relation, merge of an already-merged pair, re-assert
 * of an existing fact — all resolve to a benign op so the frame can honestly
 * report "nothing changed" and the retry stays safe.
 *
 * The struct zoo below is all plan-phase staging; the WriteReport structs
 * further down (near apply) are what actually reaches the frame. */
/* Plan/apply per plan §4: plan resolves every ref against the immutable graph
 * (find() lookups only, nothing interned) and stages ops; apply executes in
 * fixed order. M1 covers templates -> elements -> facts plus instance_of
 * typing, attrs expansion, attr-set dedup, and source/src metas; the rest of
 * the order (changes -> retract -> merge) is stubbed below. */

/* Defined beside the frame emitter (S8); the plan rejects colliding kinds. */
static int is_reserved_frame_key(const char *norm, u32 nlen);

/* Plan-phase slot value tags (apply resolves them to Term). */
enum { VT_EPEND, VT_RCONST, VT_RPEND };

typedef struct {
  u32 norm;        /* scratch-arena norm id; NONE_U32 for #id refs */
  Span surface;    /* first surface form; the mint's canonical name */
  u32 raw;         /* raw-arena string id for constructed names (current_*),
                      or NONE_U32 = the surface span holds the bytes */
  u32 existing;    /* element id, or NONE_U32 = mint */
  u8 reuse_listed; /* already in the reused_elements order */
  u8 touched;      /* reused element gets an access bump at apply */
  u8 homonym; /* tier-1 multi-match pick: occurrences report in resolution */
  u8 forced;  /* new: true mint — exempt from near_matches */
  char path[160]; /* first-touch payload path (near_matches `at`) */
} PendElem;

typedef struct {
  u32 name_pend[5];
  u8 vtag[5];
  u32 vid[5]; /* VT_EPEND: pend idx · VT_RCONST: rel id · VT_RPEND: plan-rel idx
               */
  u8 count;
  u8 status;
  u8 listed; /* base relation: shows in minted_/reused_relations */
  u8 has_salience;
  double confidence, salience;
  u32 existing;    /* rel id, or NONE_U32 = mint */
  u32 extra_bumps; /* same-payload duplicate submissions */
} PlanRel;

/* Staged ops beyond element/relation mints. Everything references pends and
 * plan-rel idxs; apply resolves them to real ids (plan §4: the graph and the
 * clock stay untouched until every ref has validated). */

typedef struct {
  u32 target_pend, curname_pend, cache_idx, to_pend;
} CurOv; /* staged cur cache */
typedef struct {
  u8 staged;
  u32 id;
  u8 status;
} FlipOp; /* -> Superseded / Retracted */
typedef struct {
  u8 staged;
  u32 id;
  double value;
} SalOp; /* graph-PE salience seed */
typedef struct {
  u32 cache_idx;
  u8 prior_staged;
  u32 prior_id;
} SupMetaOp;
typedef struct {
  u32 cache_idx, event_idx;
} DerivMetaOp;
typedef struct {
  u32 event_idx;   /* the change event, always a staged rel */
  u8 prior_staged; /* the contradicted live prior */
  u32 prior_id;    /* rel id or plan-rel idx per prior_staged */
  u8 pval_tag;     /* prior value: 0 elem id, 1 rel id, 2 pend idx */
  u32 pval_id;
  u32 property_pend, to_pend;
} ConflictOp;
enum { VIA_HOMONYM, VIA_LEXICAL };

typedef struct {
  char at[224];
  Span submitted;
  u8 is_pend;
  u8 resolved;
  u32 id;
  u8 via;
  double score;               /* tier-1 homonym: score 1; tier-2: containment */
  u32 cand_start, cand_count; /* resolved:false candidates, plan cand pool */
} ResOp;
typedef struct {
  char at[160];
  u32 pend, existing;
  double score;
} NearOp;
typedef struct {
  u32 inst_pend, kind_pend;
  u32 keys[LEGEND_LIST_CAP];
  u32 key_count; /* unexpected attr-name pends */
} DriftOp;
typedef struct {
  u32 kind_pend, attr_pend;
} StagedExpect; /* same-save template expects */
typedef struct {
  u32 from_elem, into_elem;
  Span from_span, into_span;
} MergeOp;
typedef struct {
  u32 rel_idx;
  u32 ptr_pend;
  u8 meta;
} SrcOp; /* meta=0: element form, base rel only */
typedef struct {
  u32 rel_idx; /* the fact/event relation the modals annotate */
  u32 modal;   /* MODF_* bitset */
} ModalOp;

/* The plan-phase workspace: one of these is built up during PASS 1 and then
 * consumed by apply. It owns the pend list, all the staged-op arrays, and the
 * report accumulators (resops/nears/drifts). Everything here is scratch — it
 * is reset (plan_reset) each invocation; nothing in it touches the graph. */
typedef struct {
  const Submission *sub;
  const char *buf;
  const Hypergraph *g;
  PendElem *pends;
  u32 pend_count, pend_cap;
  StrArena scratch; /* plan-local normalized forms */
  StrArena raw;     /* constructed surface forms (current_<property>, active) */
  StrMap pend_by_norm; /* scratch norm id -> first pend idx */
  U32Vec reused_order; /* pend idxs, first listable reuse touch order */
  PlanRel *rels;
  u32 rel_count, rel_cap;
  U32Vec reused_rel_order; /* plan-rel idxs, first reuse order (listed only) */
  u32 *elem_entry_pend;
  u32 elem_entry_cap; /* per elements[] entry: name pend */
  u32 *elem_entry_kind;
  u32 elem_entry_kcap; /* per elements[] entry: kind pend or NONE */
  u32 *tmpl_entry_pend;
  u32 tmpl_entry_cap; /* per templates[] entry: kind pend */
  u32 source_pend;    /* NONE_U32 when the payload has no source */
  CurOv *curov;
  u32 curov_count, curov_cap;
  FlipOp *flips;
  u32 flip_count, flip_cap;
  SalOp *sals;
  u32 sal_count, sal_cap;
  SupMetaOp *supmetas;
  u32 supmeta_count, supmeta_cap;
  DerivMetaOp *derivmetas;
  u32 derivmeta_count, derivmeta_cap;
  ConflictOp *conflicts;
  u32 conflict_count, conflict_cap;
  ResOp *resops;
  u32 resop_count, resop_cap;
  DriftOp *drifts;
  u32 drift_count, drift_cap;
  StagedExpect *sexpects;
  u32 sexpect_count, sexpect_cap;
  MergeOp *mergeops;
  u32 mergeop_count, mergeop_cap;
  SrcOp *srcops;
  u32 srcop_count, srcop_cap;
  ModalOp *modalops;
  u32 modalop_count, modalop_cap;
  U32Vec
      retract_echo; /* rel ids acknowledged in writes.retracted, echo order */
  ResOp *focus_ops;
  u32 focus_op_count, focus_op_cap; /* resolved focus refs */
  NearOp *nears;
  u32 near_count, near_cap; /* write-position near matches */
  ScoredVec cands;          /* candidate pool behind resolved:false entries */
} Plan;

static void plan_reset(Plan *pl) {
  free(pl->pends);
  str_arena_free(&pl->scratch);
  str_arena_free(&pl->raw);
  smap_free(&pl->pend_by_norm);
  free(pl->reused_order.v);
  free(pl->rels);
  free(pl->reused_rel_order.v);
  free(pl->elem_entry_pend);
  free(pl->elem_entry_kind);
  free(pl->tmpl_entry_pend);
  free(pl->curov);
  free(pl->flips);
  free(pl->sals);
  free(pl->supmetas);
  free(pl->derivmetas);
  free(pl->conflicts);
  free(pl->resops);
  free(pl->drifts);
  free(pl->sexpects);
  free(pl->mergeops);
  free(pl->srcops);
  free(pl->modalops);
  free(pl->retract_echo.v);
  free(pl->focus_ops);
  free(pl->nears);
  free(pl->cands.v);
  memset(pl, 0, sizeof *pl);
}

static u32 plan_pend_push(Plan *pl, u32 norm, Span surface, u32 raw,
                          u32 existing) {
  u32 idx = pl->pend_count;
  pl->pends =
      (PendElem *)xgrow(pl->pends, idx + 1, &pl->pend_cap, sizeof *pl->pends);
  pl->pends[idx].norm = norm;
  pl->pends[idx].surface = surface;
  pl->pends[idx].raw = raw;
  pl->pends[idx].existing = existing;
  pl->pends[idx].reuse_listed = 0;
  pl->pends[idx].touched = 0;
  pl->pends[idx].homonym = 0;
  pl->pends[idx].forced = 0;
  pl->pends[idx].path[0] = 0;
  pl->pend_count = idx + 1;
  return idx;
}

/* Mark a reuse touch; list it in reused_elements order when listable.
 * Attribute vocabulary, source, and focus touches pass listable = 0
 * (spec §7 writes field notes); mints always list, in pend order. */
static void plan_touch(Plan *pl, u32 pend, int listable) {
  PendElem *p = &pl->pends[pend];
  if (p->existing == NONE_U32)
    return;
  p->touched = 1;
  if (listable && !p->reuse_listed) {
    p->reuse_listed = 1;
    u32vec_push(&pl->reused_order, pend);
  }
}

/* A homonym-resolved pend reports once per occurrence (spec §7: resolution is
 * keyed by occurrence). Constructed names carry no surface span and are not
 * caller positions, so they never report. */
static void plan_log_homonym(Plan *pl, const char *path, Span submitted,
                             u32 pend) {
  ResOp *op;
  if (span_absent(submitted))
    return;
  pl->resops = (ResOp *)xgrow(pl->resops, pl->resop_count + 1, &pl->resop_cap,
                              sizeof *pl->resops);
  op = &pl->resops[pl->resop_count++];
  snprintf(op->at, sizeof op->at, "%s", path);
  op->submitted = submitted;
  op->is_pend = 1;
  op->resolved = 1;
  op->id = pend;
  op->via = VIA_HOMONYM;
  op->score = 1.0;
  op->cand_start = op->cand_count = 0;
}

/* must_exist reference miss: stage the closest lexical (Jaccard) matches as
 * "did you mean" candidates in the error envelope. Runs only on the error path,
 * so the full element scan is fine. */
static void stage_must_exist_cands(const Hypergraph *g, const char *q, u32 qlen) {
  static ScoredVec sv;
  static U32Vec ids;
  u32 e, n;
  sv.count = 0;
  {
    static U32Vec qtri;
    tier2_trigrams(q, qlen, &qtri);
    for (e = 0; e < g->element_count; e++) {
      double s;
      if (g->elements[e].redirect != NONE_U32)
        continue;
      s = tier2_near_score(g, &qtri, e);
      if (s >= 0.3)
        scored_push(&sv, e, s);
    }
  }
  if (sv.count > 1)
    qsort(sv.v, sv.count, sizeof *sv.v, scored_cmp);
  ids.count = 0;
  n = sv.count < TIER2_CAND_CAP ? sv.count : TIER2_CAND_CAP;
  for (e = 0; e < n; e++)
    u32vec_push(&ids, sv.v[e].elem);
  stage_error_candidates(g, ids.v, ids.count);
}

/* Resolve a name (spec §8 write-position rules) to a pend idx. The bytes are
 * either a payload span (surface) or a constructed form living in pl->raw.
 * must_exist: reference positions (changes.target, resolves.o) that must name an
 * existing element -- on a miss, error with candidates rather than mint a phantom. */
static u32 plan_name_ref(Plan *pl, const char *bytes, u32 blen, Span surface,
                         u32 raw, const char *path, u32 want_kind_pend,
                         int forced_new, int listable, int must_exist) {
  u32 nlen, nid, pend, count = 1;
  nlen = normalize_into_scratch(bytes, blen);
  if (nlen == 0)
    fail(ERR_PARSE, path, "name is empty after normalization");
  if (!forced_new) {
    pend = smap_get(&pl->pend_by_norm, &pl->scratch, g_norm_buf, nlen);
    if (pend != NONE_U32) {
      plan_touch(pl, pend, listable);
      if (pl->pends[pend].homonym)
        plan_log_homonym(pl, path, surface, pend);
      return pend;
    }
  }
  nid = str_intern(&pl->scratch, g_norm_buf, nlen);
  if (!forced_new) {
    u32 want_kind = NONE_U32;
    if (want_kind_pend != NONE_U32 &&
        pl->pends[want_kind_pend].existing != NONE_U32)
      want_kind = pl->pends[want_kind_pend].existing;
    {
      const char *norm = str_ptr(&pl->scratch, nid);
      u32 existing = resolve_tier1_n(pl->g, norm, nlen, want_kind, &count);
      if (existing != NONE_U32) {
        pend = plan_pend_push(pl, nid, surface, raw, existing);
        pl->pends[pend].homonym = count > 1;
        smap_put(&pl->pend_by_norm, &pl->scratch, nid, pend);
        plan_touch(pl, pend, listable);
        if (pl->pends[pend].homonym)
          plan_log_homonym(pl, path, surface, pend);
        return pend;
      }
    }
  }
  if (must_exist) {
    stage_must_exist_cands(pl->g, bytes, blen);
    fail(ERR_UNKNOWN_REF, path,
         "no element named \"%.*s\" -- a reference here must resolve to an "
         "existing element (recall it first, or fix the name)",
         (int)(blen > 80 ? 80 : blen), bytes);
  }
  pend = plan_pend_push(pl, nid, surface, raw, NONE_U32);
  pl->pends[pend].forced = (u8)(forced_new != 0);
  snprintf(pl->pends[pend].path, sizeof pl->pends[pend].path, "%s", path);
  if (smap_get(&pl->pend_by_norm, &pl->scratch, str_ptr(&pl->scratch, nid),
               nlen) == NONE_U32)
    smap_put(&pl->pend_by_norm, &pl->scratch, nid, pend);
  return pend;
}

/* Resolve one element ref (spec §8 write-position rules) to a pend idx.
 * must_exist: error (with candidates) on a name miss instead of minting. */
static u32 plan_ref_ex(Plan *pl, Span s, const char *path, u32 want_kind_pend,
                       int forced_new, int listable, int must_exist) {
  u32 id, pend;
  if (span_is_rel_ref(pl->buf, s))
    fail(ERR_PARSE, path, "expected an element ref, got a relation ref");
  if (span_parse_elem_id(pl->buf, s, &id)) {
    u32 i;
    if (forced_new)
      fail(ERR_PARSE, path, "new: true requires a name, not an id ref");
    if (id >= pl->g->element_count)
      fail(ERR_UNKNOWN_REF, path, "unknown element #%u", id);
    id = follow_redirect(pl->g, id);
    for (i = 0; i < pl->pend_count; i++)
      if (pl->pends[i].existing == id && pl->pends[i].norm == NONE_U32) {
        plan_touch(pl, i, listable);
        return i;
      }
    pend = plan_pend_push(pl, NONE_U32, s, NONE_U32, id);
    plan_touch(pl, pend, listable);
    return pend;
  }
  return plan_name_ref(pl, pl->buf + s.off, s.len, s, NONE_U32, path,
                       want_kind_pend, forced_new, listable, must_exist);
}

/* The common case: a reference that may mint on a miss. */
static u32 plan_ref(Plan *pl, Span s, const char *path, u32 want_kind_pend,
                    int forced_new, int listable) {
  return plan_ref_ex(pl, s, path, want_kind_pend, forced_new, listable, 0);
}

/* A constructed name (current_<property>, active): NUL-terminated bytes kept
 * in the plan's raw arena so apply can mint from them. */
static u32 plan_ref_raw(Plan *pl, const char *bytes, u32 blen, const char *path,
                        int listable) {
  u32 raw = str_intern(&pl->raw, bytes, blen);
  return plan_name_ref(pl, str_ptr(&pl->raw, raw), blen, span_none(), raw, path,
                       NONE_U32, 0, listable, 0);
}

/* A slot value: an element ref or "rel:<id>". must_exist forbids minting the
 * value element (used for resolves.o, whose object must already exist). */
static void plan_slot_value(Plan *pl, Span s, const char *path, int listable,
                            u8 *vtag, u32 *vid, int must_exist) {
  u32 id;
  if (span_parse_rel_id(pl->buf, s, &id)) {
    if (id >= pl->g->relation_count)
      fail(ERR_UNKNOWN_REF, path, "unknown relation rel:%u", id);
    *vtag = VT_RCONST;
    *vid = id;
    return;
  }
  *vtag = VT_EPEND;
  *vid = plan_ref_ex(pl, s, path, NONE_U32, 0, listable, must_exist);
}

/* Canonical plan-space slot key: collapses pends that wrap the same existing
 * element so staged dedup sees through "#42" vs the name resolving to #42. */
static void plan_slot_key(const Plan *pl, u32 name_pend, u8 vtag, u32 vid,
                          u32 out[3]) {
  u32 np = pl->pends[name_pend].existing;
  out[0] = np != NONE_U32 ? np : 0x80000000u | name_pend;
  if (vtag == VT_EPEND) {
    u32 ve = pl->pends[vid].existing;
    out[1] = 0;
    out[2] = ve != NONE_U32 ? ve : 0x80000000u | vid;
  } else if (vtag == VT_RCONST) {
    out[1] = 1;
    out[2] = vid;
  } else {
    out[1] = 2;
    out[2] = vid;
  }
}

static void plan_rel_canon(const Plan *pl, const PlanRel *r, u32 out[5][3]) {
  u32 i, j;
  for (i = 0; i < r->count; i++)
    plan_slot_key(pl, r->name_pend[i], r->vtag[i], r->vid[i], out[i]);
  for (i = 1; i < r->count; i++) {
    u32 key[3];
    memcpy(key, out[i], sizeof key);
    for (j = i; j > 0; j--) {
      if (memcmp(out[j - 1], key, sizeof key) <= 0)
        break;
      memcpy(out[j], out[j - 1], sizeof key);
    }
    memcpy(out[j], key, sizeof key);
  }
}

static int plan_rel_eq(const Plan *pl, const PlanRel *a, const PlanRel *b) {
  u32 ka[5][3], kb[5][3];
  if (a->count != b->count)
    return 0;
  plan_rel_canon(pl, a, ka);
  plan_rel_canon(pl, b, kb);
  return memcmp(ka, kb, (size_t)a->count * sizeof ka[0]) == 0;
}

/* Stage one relation: dedup against the graph (all parts existing) and
 * against already-staged relations, else append a mint. Returns the plan-rel
 * idx. Options apply on mint only; a duplicate bumps support. */
static u32 plan_relation(Plan *pl, const u32 *name_pend, const u8 *vtag,
                         const u32 *vid, u8 count, u8 status, double confidence,
                         double salience, u8 has_salience, int listed) {
  PlanRel r;
  u32 i;
  int all_existing = 1;
  memset(&r, 0, sizeof r);
  memcpy(r.name_pend, name_pend, (size_t)count * sizeof *name_pend);
  memcpy(r.vtag, vtag, (size_t)count * sizeof *vtag);
  memcpy(r.vid, vid, (size_t)count * sizeof *vid);
  r.count = count;
  r.status = status;
  r.listed = (u8)listed;
  r.confidence = confidence;
  r.salience = salience;
  r.has_salience = has_salience;
  r.existing = NONE_U32;
  for (i = 0; i < count; i++) {
    if (pl->pends[name_pend[i]].existing == NONE_U32)
      all_existing = 0;
    if (vtag[i] == VT_EPEND && pl->pends[vid[i]].existing == NONE_U32)
      all_existing = 0;
    if (vtag[i] == VT_RPEND)
      all_existing = 0;
  }
  if (all_existing) {
    Attr attrs[5];
    for (i = 0; i < count; i++) {
      attrs[i].name = pl->pends[name_pend[i]].existing;
      if (vtag[i] == VT_EPEND) {
        attrs[i].value.tag = TERM_ELEM;
        attrs[i].value.id = pl->pends[vid[i]].existing;
      } else {
        attrs[i].value.tag = TERM_REL;
        attrs[i].value.id = vid[i];
      }
    }
    r.existing = dedup_lookup_live(pl->g, attrs, count);
  }
  /* staged dedup: a same-payload duplicate folds into the earlier op */
  for (i = 0; i < pl->rel_count; i++) {
    PlanRel *p = &pl->rels[i];
    if (r.existing != NONE_U32) {
      if (p->existing != r.existing)
        continue;
    } else {
      if (p->existing != NONE_U32 || !plan_rel_eq(pl, p, &r))
        continue;
    }
    p->extra_bumps++;
    return i;
  }
  if (r.existing != NONE_U32 && r.listed)
    u32vec_push(&pl->reused_rel_order, pl->rel_count);
  pl->rels = (PlanRel *)xgrow(pl->rels, pl->rel_count + 1, &pl->rel_cap,
                              sizeof *pl->rels);
  pl->rels[pl->rel_count] = r;
  return pl->rel_count++;
}

/* A pend wrapping a known element id (well-known vocabulary in meta slots). */
static u32 plan_pend_for_elem(Plan *pl, u32 elem_id) {
  u32 i;
  for (i = 0; i < pl->pend_count; i++)
    if (pl->pends[i].existing == elem_id)
      return i;
  return plan_pend_push(pl, NONE_U32, span_none(), NONE_U32, elem_id);
}

/* plan_precise_elem — precise-position resolution with same-save visibility
 * (pin 24): a name bound earlier in this payload's walk resolves through its
 * pend; a still-unminted binding cannot name anything in the graph, so a
 * retract/merge against it is unknown_ref. */
static u32 plan_precise_elem(Plan *pl, Span s, const char *path) {
  u32 tmp;
  if (!span_parse_elem_id(pl->buf, s, &tmp) && !span_is_rel_ref(pl->buf, s)) {
    u32 nlen = normalize_into_scratch(pl->buf + s.off, s.len);
    if (nlen == 0)
      fail(ERR_PARSE, path, "name is empty after normalization");
    {
      u32 pend = smap_get(&pl->pend_by_norm, &pl->scratch, g_norm_buf, nlen);
      if (pend != NONE_U32) {
        if (pl->pends[pend].existing == NONE_U32)
          fail(ERR_UNKNOWN_REF, path,
               "\"%.*s\" is minted by this save and cannot resolve yet",
               (int)(s.len > 80 ? 80 : s.len), pl->buf + s.off);
        return pl->pends[pend].existing;
      }
    }
  }
  return resolve_precise_elem(pl->g, pl->buf, s, path);
}

/* Kind lookahead for homonym preference: the name ref resolves before the
 * kind in first-touch order (plan §3.17), but prefer-kind needs the kind's
 * element first. Read-only; plan_ref later reaches the same answer. */
static u32 plan_peek_kind(Plan *pl, Span s, const char *path) {
  u32 id, nlen, pend;
  if (span_parse_elem_id(pl->buf, s, &id)) {
    if (id >= pl->g->element_count)
      fail(ERR_UNKNOWN_REF, path, "unknown element #%u", id);
    return follow_redirect(pl->g, id);
  }
  if (span_is_rel_ref(pl->buf, s))
    fail(ERR_PARSE, path, "expected an element ref, got a relation ref");
  nlen = normalize_into_scratch(pl->buf + s.off, s.len);
  if (nlen == 0)
    fail(ERR_PARSE, path, "name is empty after normalization");
  pend = smap_get(&pl->pend_by_norm, &pl->scratch, g_norm_buf, nlen);
  if (pend != NONE_U32)
    return pl->pends[pend].existing; /* may be NONE_U32: a pending mint */
  return resolve_tier1(pl->g, g_norm_buf, nlen, NONE_U32);
}

typedef struct {
  u8 tag;
  u32 id;
} PlanVal; /* vtag/vid pair for one slot value */

/* ---- staged-op push helpers ---- */

static void plan_push_flip(Plan *pl, u8 staged, u32 id, u8 status) {
  u32 i;
  for (i = 0; i < pl->flip_count; i++)
    if (pl->flips[i].staged == staged && pl->flips[i].id == id)
      return;
  pl->flips = (FlipOp *)xgrow(pl->flips, pl->flip_count + 1, &pl->flip_cap,
                              sizeof *pl->flips);
  pl->flips[pl->flip_count].staged = staged;
  pl->flips[pl->flip_count].id = id;
  pl->flips[pl->flip_count].status = status;
  pl->flip_count++;
}

static int plan_flip_staged_already(const Plan *pl, u8 staged, u32 id) {
  u32 i;
  for (i = 0; i < pl->flip_count; i++)
    if (pl->flips[i].staged == staged && pl->flips[i].id == id)
      return 1;
  return 0;
}

static void plan_push_sal(Plan *pl, u8 staged, u32 id, double value) {
  pl->sals = (SalOp *)xgrow(pl->sals, pl->sal_count + 1, &pl->sal_cap,
                            sizeof *pl->sals);
  pl->sals[pl->sal_count].staged = staged;
  pl->sals[pl->sal_count].id = id;
  pl->sals[pl->sal_count].value = value;
  pl->sal_count++;
}

static void plan_push_supmeta(Plan *pl, u32 cache_idx, u8 prior_staged,
                              u32 prior_id) {
  pl->supmetas = (SupMetaOp *)xgrow(pl->supmetas, pl->supmeta_count + 1,
                                    &pl->supmeta_cap, sizeof *pl->supmetas);
  pl->supmetas[pl->supmeta_count].cache_idx = cache_idx;
  pl->supmetas[pl->supmeta_count].prior_staged = prior_staged;
  pl->supmetas[pl->supmeta_count].prior_id = prior_id;
  pl->supmeta_count++;
}

static void plan_push_derivmeta(Plan *pl, u32 cache_idx, u32 event_idx) {
  pl->derivmetas =
      (DerivMetaOp *)xgrow(pl->derivmetas, pl->derivmeta_count + 1,
                           &pl->derivmeta_cap, sizeof *pl->derivmetas);
  pl->derivmetas[pl->derivmeta_count].cache_idx = cache_idx;
  pl->derivmetas[pl->derivmeta_count].event_idx = event_idx;
  pl->derivmeta_count++;
}

static void plan_push_srcop(Plan *pl, u32 rel_idx, u32 ptr_pend, u8 meta) {
  pl->srcops = (SrcOp *)xgrow(pl->srcops, pl->srcop_count + 1, &pl->srcop_cap,
                              sizeof *pl->srcops);
  pl->srcops[pl->srcop_count].rel_idx = rel_idx;
  pl->srcops[pl->srcop_count].ptr_pend = ptr_pend;
  pl->srcops[pl->srcop_count].meta = meta;
  pl->srcop_count++;
}

static void plan_push_modalop(Plan *pl, u32 rel_idx, u32 modal) {
  if (!modal)
    return;
  pl->modalops = (ModalOp *)xgrow(pl->modalops, pl->modalop_count + 1,
                                  &pl->modalop_cap, sizeof *pl->modalops);
  pl->modalops[pl->modalop_count].rel_idx = rel_idx;
  pl->modalops[pl->modalop_count].modal = modal;
  pl->modalop_count++;
}

/* ---- changes: the supersession form (spec §5) ---- */

/* The live prior cache for a (target, current_<property>) pair — the staged
 * overlay first (same-save chains), then the graph's cur index. */
typedef struct {
  int found, staged;
  u32 cache;  /* rel id or plan-rel idx */
  u8 val_tag; /* graph prior value: 0 element, 1 relation */
  u32 val_id; /* graph: term id · staged: the cache's to pend */
} Prior;

static void plan_curov_push(Plan *pl, u32 target_pend, u32 curname_pend,
                            u32 cache_idx, u32 to_pend) {
  pl->curov = (CurOv *)xgrow(pl->curov, pl->curov_count + 1, &pl->curov_cap,
                             sizeof *pl->curov);
  pl->curov[pl->curov_count].target_pend = target_pend;
  pl->curov[pl->curov_count].curname_pend = curname_pend;
  pl->curov[pl->curov_count].cache_idx = cache_idx;
  pl->curov[pl->curov_count].to_pend = to_pend;
  pl->curov_count++;
}

/* Two plan refs name the same node when they are the same pend or both wrap
 * the same existing element (a "#42" pend and a name pend can coexist). */
static int plan_pend_same(const Plan *pl, u32 a, u32 b) {
  if (a == b)
    return 1;
  return pl->pends[a].existing != NONE_U32 &&
         pl->pends[a].existing == pl->pends[b].existing;
}

/* Pin 25 heal: a state-flipping change also supersedes live plain relations
 * {subject: target, <property>: *} — an attr written before its property
 * started changing must not linger as a second contradictory answer. Two
 * sources heal: graph relations (an attr from an earlier save, both refs must
 * exist for one to be live) and this payload's already-staged relations (a
 * plain fact written beside the change; facts walk before changes). The staged
 * scan keys on pends, so it heals even when the property is minted this tick.
 */
static void plan_heal_plain_attrs(Plan *pl, u32 target_pend, u32 property_pend,
                                  u32 cache_idx) {
  u32 t = pl->pends[target_pend].existing;
  u32 p = pl->pends[property_pend].existing;
  u32 r, matches = 0, only = NONE_U32;
  if (t != NONE_U32 && p != NONE_U32) {
    u32 link = pl->g->rels_by_elem[t];
    while (link != NONE_U32) {
      u32 rid = pl->g->rel_links[link].rel;
      link = pl->g->rel_links[link].next;
      {
        const Relation *rel = &pl->g->relations[rid];
        int subj_is_target = 0, prop_named = 0;
        u32 a;
        if (rel->status >= ST_SUPERSEDED || rel->attr_count != 2)
          continue;
        for (a = 0; a < 2; a++) {
          if (rel->attrs[a].name == WK_SUBJECT &&
              rel->attrs[a].value.tag == TERM_ELEM &&
              rel->attrs[a].value.id == t)
            subj_is_target = 1;
          if (rel->attrs[a].name == p && rel->attrs[a].name != WK_SUBJECT)
            prop_named = 1;
        }
        if (!subj_is_target || !prop_named)
          continue;
        matches++;
        only = rid;
      }
    }
    /* Several live values means the property is multi-valued on this subject,
     * and one change does not speak for all of them -- superseding the set drops
     * scopes with no record that it happened. Widening a multi-valued property
     * is retract's job. rel:0..9 are the ontology templates, whose `expects`
     * edges retract already refuses to touch. */
    if (matches == 1 && only >= WK_RELATION_COUNT &&
        !plan_flip_staged_already(pl, 0, only)) {
      plan_push_flip(pl, 0, only, ST_SUPERSEDED);
      plan_push_supmeta(pl, cache_idx, 0, only);
    }
  }
  matches = 0;
  only = NONE_U32;
  for (r = 0; r < pl->rel_count; r++) {
    PlanRel *pr = &pl->rels[r];
    int subj_is_target = 0, prop_named = 0;
    u32 a;
    if (pr->count != 2)
      continue;
    for (a = 0; a < 2; a++) {
      if (pl->pends[pr->name_pend[a]].existing == WK_SUBJECT &&
          pr->vtag[a] == VT_EPEND &&
          plan_pend_same(pl, pr->vid[a], target_pend))
        subj_is_target = 1;
      if (pl->pends[pr->name_pend[a]].existing != WK_SUBJECT &&
          plan_pend_same(pl, pr->name_pend[a], property_pend))
        prop_named = 1;
    }
    if (!subj_is_target || !prop_named)
      continue;
    matches++;
    only = r;
  }
  if (matches == 1 && !plan_flip_staged_already(pl, 1, only)) {
    plan_push_flip(pl, 1, only, ST_SUPERSEDED);
    plan_push_supmeta(pl, cache_idx, 1, only);
  }
}

/* Kind correction: resubmitting an existing element with a different `kind`
 * supersedes its old live instance_of so exactly one kind stays live (the
 * latest). Without this the old instance_of lingers and the derived elem_kind
 * cannot move — the "immutable kind" trial issue. */
static void plan_supersede_old_kind(Plan *pl, u32 elem, u32 newk) {
  u32 link, cur;
  if (elem == NONE_U32)
    return;
  cur = pl->g->elem_kind[elem];
  /* newk == NONE_U32 means the new kind is minted THIS tick -- still a change,
   * since the current kind is an existing id. Skip only when truly unchanged. */
  if (cur == NONE_U32 || cur == newk)
    return; /* newly minted element, or kind unchanged */
  link = pl->g->rels_by_elem[elem];
  while (link != NONE_U32) {
    u32 rid = pl->g->rel_links[link].rel, a;
    const Relation *r = &pl->g->relations[rid];
    int subj = 0, inst = 0;
    link = pl->g->rel_links[link].next;
    if (r->status >= ST_SUPERSEDED || r->attr_count != 2)
      continue;
    for (a = 0; a < 2; a++) {
      if (r->attrs[a].name == WK_SUBJECT && r->attrs[a].value.tag == TERM_ELEM &&
          r->attrs[a].value.id == elem)
        subj = 1;
      if (r->attrs[a].name == WK_INSTANCE_OF)
        inst = 1;
    }
    if (subj && inst && !plan_flip_staged_already(pl, 0, rid))
      plan_push_flip(pl, 0, rid, ST_SUPERSEDED);
  }
}

/* The shared state-flip core for changes and event-shaped facts — this is
 * SUPERSESSION in code. Given the new value (to_pend) and the existing current
 * value (prior, already looked up), there are three outcomes:
 *   - same value: nothing to flip; just refresh the cache.
 *   - a DIFFERENT prior exists but the confidence gate failed (and this write
 *     did not `intervened`): a CONFLICT. The new value does NOT win; the store
 *     keeps the old one, records the clash for the frame, and seeds the
 *     contradicted prior with the highest salience (it is exactly what should
 *     surface next). Returns NONE_U32 — no new cache.
 *   - a different prior exists and the gate passed (or no prior): FLIP the
 *     prior to Superseded and mint a new `current_<property>` cache relation,
 *     wiring up the supersedes/derived_from metas and healing any stale plain
 *     attrs (pin 25). The cache carries the new current value; history keeps
 *     the old.
 * Returns the plan-rel idx of the current cache (mint or reuse). */
static u32 plan_change_state(Plan *pl, u32 event_idx, u32 target_pend,
                             u32 property_pend, u32 curname_pend, u32 to_pend,
                             const Prior *prior, double conf, int gate_passed) {
  u32 cache_idx;
  int same_value = 0;
  int flipped_prior = 0;
  if (prior->found) {
    if (prior->staged) {
      u32 pv = prior->val_id; /* to pend of the staged cache */
      same_value = pv == to_pend ||
                   (pl->pends[pv].existing != NONE_U32 &&
                    pl->pends[pv].existing == pl->pends[to_pend].existing);
    } else if (prior->val_tag == TERM_ELEM) {
      same_value = pl->pends[to_pend].existing != NONE_U32 &&
                   pl->pends[to_pend].existing == prior->val_id;
    }
  }
  if (prior->found && !same_value && !gate_passed) {
    /* conflict: the write does not flip state (spec §7); the contradicted
     * prior gets the highest graph-PE salience seed (plan §3.5) */
    ConflictOp *op;
    pl->conflicts =
        (ConflictOp *)xgrow(pl->conflicts, pl->conflict_count + 1,
                            &pl->conflict_cap, sizeof *pl->conflicts);
    op = &pl->conflicts[pl->conflict_count++];
    op->event_idx = event_idx;
    op->prior_staged = (u8)prior->staged;
    op->prior_id = prior->cache;
    op->pval_tag = prior->staged ? 2 : prior->val_tag;
    op->pval_id = prior->val_id;
    op->property_pend = property_pend;
    op->to_pend = to_pend;
    plan_push_sal(pl, (u8)prior->staged, prior->cache,
                  pl->g->policy.salience_conflict);
    return NONE_U32;
  }
  if (prior->found && !same_value) {
    plan_push_flip(pl, (u8)prior->staged, prior->cache, ST_SUPERSEDED);
    flipped_prior = 1;
  }
  {
    u32 names[2], vids[2];
    u8 tags[2];
    double sal = flipped_prior ? pl->g->policy.salience_supersession : 0.0;
    names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
    tags[0] = VT_EPEND;
    vids[0] = target_pend;
    names[1] = curname_pend;
    tags[1] = VT_EPEND;
    vids[1] = to_pend;
    cache_idx = plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, conf, sal,
                              (u8)flipped_prior, 1);
  }
  if (flipped_prior)
    plan_push_supmeta(pl, cache_idx, (u8)prior->staged, prior->cache);
  plan_push_derivmeta(pl, cache_idx, event_idx);
  if (!same_value && gate_passed)
    plan_heal_plain_attrs(pl, target_pend, property_pend, cache_idx);
  plan_curov_push(pl, target_pend, curname_pend, cache_idx, to_pend);
  return cache_idx;
}

/* current_<property> is literal (plan §3.3): "current_" + the property
 * element's first name (its canonical form when it already exists, the
 * submitted surface when this save mints it). Returns a shared scratch
 * pointer valid until the next call. */
static const char *plan_current_name(Plan *pl, u32 property_pend,
                                     u32 *out_len) {
  static char *buf;
  static u32 buf_cap;
  const PendElem *p = &pl->pends[property_pend];
  const char *pname;
  u32 plen;
  if (p->existing != NONE_U32) {
    pname = elem_name(pl->g, p->existing);
    plen = elem_name_l(pl->g, p->existing);
  } else if (p->raw != NONE_U32) {
    pname = str_ptr(&pl->raw, p->raw);
    plen = str_len(&pl->raw, p->raw);
  } else {
    pname = pl->buf + p->surface.off;
    plen = p->surface.len;
  }
  buf = (char *)xgrow(buf, plen + 9, &buf_cap, 1);
  memcpy(buf, "current_", 8);
  memcpy(buf + 8, pname, plen);
  *out_len = plen + 8;
  return buf;
}

static u32 plan_curname_pend(Plan *pl, u32 property_pend, const char *path) {
  u32 len;
  const char *name = plan_current_name(pl, property_pend, &len);
  return plan_ref_raw(pl, name, len, path, 0);
}

/* A staged relation as a meta slot value: the real id when it dedup-reused,
 * else the plan idx apply resolves after minting. */
static void plan_rel_slot(const Plan *pl, u8 *tag, u32 *vid, u32 plan_idx) {
  if (pl->rels[plan_idx].existing != NONE_U32) {
    *tag = VT_RCONST;
    *vid = pl->rels[plan_idx].existing;
  } else {
    *tag = VT_RPEND;
    *vid = plan_idx;
  }
}

/* Template drift (spec §7): attributes an instance carries that its kind's
 * template doesn't expect. A kind with no expects vocabulary — graph or
 * staged this save — has no template, so nothing drifts. */
static void plan_check_drift(Plan *pl, u32 name_pend, u32 kind_pend,
                             const u32 *attr_keys, u32 key_count) {
  static U32Vec expects;
  u32 kind_elem = pl->pends[kind_pend].existing;
  u32 a, s, link;
  int has_template = 0;
  DriftOp op;
  memset(&op, 0, sizeof op);
  expects.count = 0;
  if (kind_elem != NONE_U32) {
    link = pl->g->rels_by_elem[kind_elem];
    while (link != NONE_U32) {
      u32 rid = pl->g->rel_links[link].rel;
      link = pl->g->rel_links[link].next;
      {
        const Relation *r = &pl->g->relations[rid];
        int subj_is_kind = 0;
        u32 exp_val = NONE_U32;
        if (r->status >= ST_SUPERSEDED || r->attr_count != 2)
          continue;
        for (a = 0; a < 2; a++) {
          if (r->attrs[a].name == WK_SUBJECT &&
              r->attrs[a].value.tag == TERM_ELEM &&
              r->attrs[a].value.id == kind_elem)
            subj_is_kind = 1;
          if (r->attrs[a].name == WK_EXPECTS &&
              r->attrs[a].value.tag == TERM_ELEM)
            exp_val = r->attrs[a].value.id;
        }
        if (subj_is_kind && exp_val != NONE_U32)
          u32vec_push(&expects, exp_val);
      }
    }
  }
  if (expects.count)
    has_template = 1;
  for (s = 0; s < pl->sexpect_count; s++) {
    const StagedExpect *se = &pl->sexpects[s];
    if (se->kind_pend == kind_pend ||
        (kind_elem != NONE_U32 &&
         pl->pends[se->kind_pend].existing == kind_elem))
      has_template = 1;
  }
  if (!has_template)
    return;
  for (a = 0; a < key_count; a++) {
    u32 kp = attr_keys[a];
    u32 ke = pl->pends[kp].existing;
    u32 e, d;
    int member = 0, dup = 0;
    if (ke != NONE_U32)
      for (e = 0; e < expects.count; e++)
        if (expects.v[e] == ke)
          member = 1;
    for (s = 0; s < pl->sexpect_count && !member; s++) {
      const StagedExpect *se = &pl->sexpects[s];
      int kind_ok = se->kind_pend == kind_pend ||
                    (kind_elem != NONE_U32 &&
                     pl->pends[se->kind_pend].existing == kind_elem);
      int attr_ok = se->attr_pend == kp ||
                    (ke != NONE_U32 && pl->pends[se->attr_pend].existing == ke);
      if (kind_ok && attr_ok)
        member = 1;
    }
    if (member)
      continue;
    for (d = 0; d < op.key_count; d++)
      if (op.keys[d] == kp)
        dup = 1;
    if (!dup && op.key_count < LEGEND_LIST_CAP)
      op.keys[op.key_count++] = kp;
  }
  if (op.key_count == 0)
    return;
  op.inst_pend = name_pend;
  op.kind_pend = kind_pend;
  pl->drifts = (DriftOp *)xgrow(pl->drifts, pl->drift_count + 1, &pl->drift_cap,
                                sizeof *pl->drifts);
  pl->drifts[pl->drift_count++] = op;
}

/* Read-only peek at the current_<property> name: enough to find the prior
 * without minting the name pend early (the pend joins first-touch order after
 * the change's own from/to refs — the walk the fixtures pin). */
typedef struct {
  u32 pend;
  u32 elem;
} CurNamePeek;

static CurNamePeek plan_curname_peek(Plan *pl, u32 property_pend) {
  CurNamePeek out;
  u32 len, nlen;
  const char *name = plan_current_name(pl, property_pend, &len);
  out.pend = NONE_U32;
  out.elem = NONE_U32;
  nlen = normalize_into_scratch(name, len);
  if (nlen == 0)
    return out;
  out.pend = smap_get(&pl->pend_by_norm, &pl->scratch, g_norm_buf, nlen);
  if (out.pend != NONE_U32)
    out.elem = pl->pends[out.pend].existing;
  else
    out.elem = resolve_tier1(pl->g, g_norm_buf, nlen, NONE_U32);
  return out;
}

static void plan_prior_peek(const Plan *pl, u32 target_pend, CurNamePeek cn,
                            Prior *out) {
  memset(out, 0, sizeof *out);
  if (cn.pend != NONE_U32) {
    u32 i;
    for (i = pl->curov_count; i > 0; i--) {
      const CurOv *ov = &pl->curov[i - 1];
      if (ov->target_pend == target_pend && ov->curname_pend == cn.pend) {
        out->found = 1;
        out->staged = 1;
        out->cache = ov->cache_idx;
        out->val_id = ov->to_pend;
        return;
      }
    }
  }
  if (pl->pends[target_pend].existing == NONE_U32 || cn.elem == NONE_U32)
    return;
  {
    u32 rel = cur_get_live(pl->g, pl->pends[target_pend].existing, cn.elem);
    u32 a;
    if (rel == NONE_U32)
      return;
    out->found = 1;
    out->cache = rel;
    for (a = 0; a < pl->g->relations[rel].attr_count; a++) {
      const Attr *at = &pl->g->relations[rel].attrs[a];
      if (at->name != WK_SUBJECT) {
        out->val_tag = at->value.tag;
        out->val_id = at->value.id;
      }
    }
  }
}

/* Precise term for a retract fact shape: rel:<id> or an element ref. */
static Term plan_precise_term(Plan *pl, Span s, const char *path) {
  Term t;
  u32 id;
  if (span_parse_rel_id(pl->buf, s, &id)) {
    if (id >= pl->g->relation_count)
      fail(ERR_UNKNOWN_REF, path, "unknown relation rel:%u", id);
    t.tag = TERM_REL;
    t.id = id;
    return t;
  }
  t.tag = TERM_ELEM;
  t.id = plan_precise_elem(pl, s, path);
  return t;
}

/* Locate a retract fact shape among live relations only (spec §5, pin §3.16):
 * every slot resolves precisely, then the attr-set dedup hash answers. */
static u32 plan_resolve_fact_shape(Plan *pl, const SubFact *f,
                                   const char *path) {
  Attr attrs[5];
  u8 n = 0;
  u32 rid;
  char vpath[304];
  if (f->is_triple) {
    snprintf(vpath, sizeof vpath, "%s.s", path);
    attrs[0].name = WK_SUBJECT;
    attrs[0].value = plan_precise_term(pl, f->s, vpath);
    snprintf(vpath, sizeof vpath, "%s.p", path);
    attrs[1].name = plan_precise_elem(pl, f->p, vpath);
    snprintf(vpath, sizeof vpath, "%s.o", path);
    attrs[1].value = plan_precise_term(pl, f->o, vpath);
    n = 2;
  } else {
    u32 a;
    for (a = 0; a < f->attr_count; a++) {
      const AttrEntry *ae = &pl->sub->attr_pool[f->attr_start + a];
      u32 klen = ae->key.len > 64 ? 64 : ae->key.len;
      if (ae->val_count != 1)
        fail(ERR_PARSE, path, "a retract fact shape takes single values");
      snprintf(vpath, sizeof vpath, "%s.%.*s", path, (int)klen,
               pl->buf + ae->key.off);
      attrs[n].name = plan_precise_elem(pl, ae->key, vpath);
      attrs[n].value =
          plan_precise_term(pl, pl->sub->span_pool[ae->val_start], vpath);
      n++;
    }
  }
  rid = dedup_lookup_live(pl->g, attrs, n);
  if (rid == NONE_U32)
    fail(ERR_UNKNOWN_REF, path, "no live relation matches this fact shape");
  return rid;
}

/* Fixed walk order (plan §3.17): templates -> elements -> facts -> changes ->
 * retract -> merge, then the metas phase (instance_of typing, change metas,
 * src pointers, source provenance) and focus (read positions, never mint). */
/* `current_<property>` is the namespace the change path constructs for its value
 * cache. A plain fact written there sits beside the cache rather than
 * superseding it, and the audit's status-flavor check skips the prefix on the
 * grounds that a current_* name is one `changes` already wrote -- so two
 * contradicting values live in the state band with every check reporting clean. */
static void reject_cache_predicate(Plan *pl, Span s, const char *path) {
  const char *b = pl->buf + s.off;
  static const char *const PFX = "current_";
  u32 i;
  if (s.len <= 8)
    return;
  for (i = 0; i < 8; i++) {
    char c = b[i];
    if (c >= 'A' && c <= 'Z')
      c = (char)(c - 'A' + 'a');
    if (c != PFX[i])
      return;
  }
  fail(ERR_PARSE, path,
       "\"%.*s\" is the value cache the change path owns -- write "
       "changes {target, property: \"%.*s\", to} instead, which supersedes the "
       "prior value and keeps the history",
       (int)(s.len > 64 ? 64 : s.len), b,
       (int)(s.len - 8 > 56 ? 56 : s.len - 8), b + 8);
}

/* Words in a name: runs of alphanumerics. The element name and the kind slot
 * are both nouns, and both carry a word cap. */
static u32 name_word_count(const char *s, u32 len) {
  u32 i, words = 0;
  int inword = 0;
  for (i = 0; i < len; i++) {
    char c = s[i];
    int alnum = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                (c >= '0' && c <= '9');
    if (alnum) {
      if (!inword) {
        words++;
        inword = 1;
      }
    } else {
      inword = 0;
    }
  }
  return words;
}

static void plan_submission(Plan *pl, const Hypergraph *g,
                            const Submission *sub, const char *buf) {
  char path[224];
  u32 i;

  pl->sub = sub;
  pl->buf = buf;
  pl->g = g;
  pl->source_pend = NONE_U32;

  /* -- templates: kind element + one {subject, expects} edge per name -- */
  pl->tmpl_entry_pend = (u32 *)xgrow(
      pl->tmpl_entry_pend, sub->template_count ? sub->template_count : 1,
      &pl->tmpl_entry_cap, sizeof *pl->tmpl_entry_pend);
  for (i = 0; i < sub->template_count; i++) {
    const SubTemplate *t = &sub->templates[i];
    u32 kind_pend, e;
    u32 knlen = normalize_into_scratch(buf + t->kind.off, t->kind.len);
    snprintf(path, sizeof path, "templates[%u].kind", i);
    if (is_reserved_frame_key(g_norm_buf, knlen))
      fail(ERR_PARSE, path, "reserved frame section name");
    kind_pend = plan_ref(pl, t->kind, path, NONE_U32, 0, 1);
    pl->tmpl_entry_pend[i] = kind_pend;
    for (e = 0; e < t->expects_count; e++) {
      u32 attr_pend;
      u32 names[2], vids[2];
      u8 tags[2];
      snprintf(path, sizeof path, "templates[%u].expects[%u]", i, e);
      attr_pend = plan_ref(pl, sub->span_pool[t->expects_start + e], path,
                           NONE_U32, 0, 0);
      names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
      tags[0] = VT_EPEND;
      vids[0] = kind_pend;
      names[1] = plan_pend_for_elem(pl, WK_EXPECTS);
      tags[1] = VT_EPEND;
      vids[1] = attr_pend;
      plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 1);
      pl->sexpects =
          (StagedExpect *)xgrow(pl->sexpects, pl->sexpect_count + 1,
                                &pl->sexpect_cap, sizeof *pl->sexpects);
      pl->sexpects[pl->sexpect_count].kind_pend = kind_pend;
      pl->sexpects[pl->sexpect_count].attr_pend = attr_pend;
      pl->sexpect_count++;
    }
  }

  /* -- elements: name (kind-preferring), kind, aliases, attrs expansion -- */
  pl->elem_entry_pend = (u32 *)xgrow(
      pl->elem_entry_pend, sub->element_count ? sub->element_count : 1,
      &pl->elem_entry_cap, sizeof *pl->elem_entry_pend);
  pl->elem_entry_kind = (u32 *)xgrow(
      pl->elem_entry_kind, sub->element_count ? sub->element_count : 1,
      &pl->elem_entry_kcap, sizeof *pl->elem_entry_kind);
  for (i = 0; i < sub->element_count; i++) {
    const SubElement *el = &sub->elements[i];
    u32 name_pend, kind_pend = NONE_U32, want_kind = NONE_U32, a;
    if (!span_absent(el->kind)) {
      u32 knlen = normalize_into_scratch(buf + el->kind.off, el->kind.len);
      snprintf(path, sizeof path, "elements[%u].kind", i);
      if (is_reserved_frame_key(g_norm_buf, knlen))
        fail(ERR_PARSE, path, "reserved frame section name");
      want_kind = plan_peek_kind(pl, el->kind, path);
    }
    snprintf(path, sizeof path, "elements[%u].name", i);
    if (want_kind != NONE_U32) {
      /* borrow a throwaway pend so plan_ref sees the kind element */
      u32 wk_pend = plan_pend_for_elem(pl, want_kind);
      name_pend = plan_ref(pl, el->name, path, wk_pend, el->is_new, 1);
    } else {
      name_pend = plan_ref(pl, el->name, path, NONE_U32, el->is_new, 1);
    }
    /* Name cap (entity-model): a NAME is a short noun for one thing. Reject a name
     * that would MINT a new element and runs over 5 words -- that reads as a claim
     * or a log, not an entity. Mint-only (a long name that resolves to an existing
     * element is a legacy reference, not a new one). The rejected attempt is captured
     * verbatim in the journal for analysis. */
    if (pl->pends[name_pend].existing == NONE_U32) {
      u32 words = name_word_count(buf + el->name.off, el->name.len);
      if (words > 5)
        fail(ERR_PROSE_VALUE, path,
             "name is %u words -- a NAME is a short noun (<=5 words) for ONE thing. "
             "This reads as a claim or a log: name the entities (nouns) and write the "
             "claim as a fact; put detail in the summary.",
             words);
    }
    if (!span_absent(el->kind)) {
      /* A KIND is a one-word noun -- every kind a real store carries (spell,
       * mechanic, constraint, module) is a single word. The cap is not mint-only
       * like the name cap: resubmitting an element with a multi-word kind
       * supersedes its real kind, and an element whose kind is a claim drops out
       * of every kind-keyed check and recall band. */
      u32 kwords = name_word_count(buf + el->kind.off, el->kind.len);
      snprintf(path, sizeof path, "elements[%u].kind", i);
      if (kwords > 2)
        fail(ERR_PROSE_VALUE, path,
             "kind is %u words -- a KIND is a one-word noun (spell, mechanic, "
             "constraint, module). This reads as a claim: keep the kind a noun and "
             "put the claim in the summary or write it as a fact.",
             kwords);
      kind_pend = plan_ref(pl, el->kind, path, NONE_U32, 0, 1);
    }
    pl->elem_entry_pend[i] = name_pend;
    pl->elem_entry_kind[i] = kind_pend;
    /* aliases and rename_to bind their norms to this pend so later refs
     * in the same save resolve through them (pin 24) */
    for (a = 0; a < el->alias_count; a++) {
      Span al = sub->span_pool[el->alias_start + a];
      u32 nlen = normalize_into_scratch(buf + al.off, al.len);
      if (nlen == 0) {
        snprintf(path, sizeof path, "elements[%u].aliases[%u]", i, a);
        fail(ERR_PARSE, path, "name is empty after normalization");
      }
      if (smap_get(&pl->pend_by_norm, &pl->scratch, g_norm_buf, nlen) ==
          NONE_U32)
        smap_put(&pl->pend_by_norm, &pl->scratch,
                 str_intern(&pl->scratch, g_norm_buf, nlen), name_pend);
    }
    if (!span_absent(el->rename_to)) {
      u32 nlen =
          normalize_into_scratch(buf + el->rename_to.off, el->rename_to.len);
      if (nlen == 0) {
        snprintf(path, sizeof path, "elements[%u].rename_to", i);
        fail(ERR_PARSE, path, "name is empty after normalization");
      }
      /* the core vocabulary's canonical names are load-bearing: the
       * snapshot reader verifies elements 0..31 by name, so a rename
       * here would brick the store it just saved */
      if (pl->pends[name_pend].existing != NONE_U32 &&
          is_core_vocab(pl->g, pl->pends[name_pend].existing)) {
        snprintf(path, sizeof path, "elements[%u].rename_to", i);
        fail(ERR_PARSE, path, "cannot rename a core vocabulary element");
      }
      if (smap_get(&pl->pend_by_norm, &pl->scratch, g_norm_buf, nlen) ==
          NONE_U32)
        smap_put(&pl->pend_by_norm, &pl->scratch,
                 str_intern(&pl->scratch, g_norm_buf, nlen), name_pend);
    }
    {
      u32 attr_keys[LEGEND_LIST_CAP];
      u32 seed_standing = NONE_U32;
      int mints_constraint = kind_pend != NONE_U32 &&
                             pl->pends[kind_pend].existing == WK_KIND_CONSTRAINT &&
                             pl->pends[name_pend].existing == NONE_U32;
      for (a = 0; a < el->attr_count; a++) {
        const AttrEntry *ae = &sub->attr_pool[el->attr_start + a];
        u32 key_pend, v;
        snprintf(path, sizeof path, "elements[%u].attrs.%.*s", i,
                 (int)(ae->key.len > 64 ? 64 : ae->key.len), buf + ae->key.off);
        reject_cache_predicate(pl, ae->key, path);
        key_pend = plan_ref(pl, ae->key, path, NONE_U32, 0, 0);
        attr_keys[a] = key_pend;
        for (v = 0; v < ae->val_count; v++) {
          u32 names[2], vids[2];
          u8 tags[2];
          names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
          tags[0] = VT_EPEND;
          vids[0] = name_pend;
          names[1] = key_pend;
          plan_slot_value(pl, sub->span_pool[ae->val_start + v], path, 1,
                          &tags[1], &vids[1], 0);
          /* a new constraint's standing seeds the current_standing cache below.
           * Writing it as a plain fact as well leaves a second copy of a value
           * that changes, and only the cache supersedes. */
          if (mints_constraint && ae->val_count == 1 && tags[1] == VT_EPEND &&
              pl->pends[key_pend].existing == WK_STANDING) {
            seed_standing = vids[1];
            continue;
          }
          plan_relation(pl, names, tags, vids, 2, ST_ASSERTED,
                        sub->intent[INTENT_CONVICTION], 0.0, 0, 1);
        }
      }
      if (kind_pend != NONE_U32 && el->attr_count > 0)
        plan_check_drift(pl, name_pend, kind_pend, attr_keys, el->attr_count);
      /* minting a constraint-kind element mints its current_standing cache, so
       * lifting it later is a real supersession (spec §5). The value comes from
       * the caller's `standing` when it supplied one -- hardcoding `active`
       * contradicts a constraint declared retired at mint, and
       * constraint_is_active reads only the cache. */
      if (mints_constraint) {
        u32 curstand, cache;
        u32 names[2], vids[2];
        u8 tags[2];
        snprintf(path, sizeof path, "elements[%u].kind", i);
        curstand = plan_ref_raw(pl, "current_standing", 16, path, 0);
        if (seed_standing == NONE_U32)
          seed_standing = plan_ref_raw(pl, "active", 6, path, 0);
        names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
        tags[0] = VT_EPEND;
        vids[0] = name_pend;
        names[1] = curstand;
        tags[1] = VT_EPEND;
        vids[1] = seed_standing;
        cache = plan_relation(pl, names, tags, vids, 2, ST_ASSERTED,
                              g->policy.default_confidence, 0.0, 0, 1);
        plan_curov_push(pl, name_pend, curstand, cache, seed_standing);
      }
    }
    /* element-form src (pin 26): a {subject, src: pointer} base relation;
     * the pointers section aggregates both forms */
    if (!span_absent(el->src)) {
      u32 ptr, rel;
      u32 names[2], vids[2];
      u8 tags[2];
      snprintf(path, sizeof path, "elements[%u].src", i);
      ptr = plan_ref(pl, el->src, path, NONE_U32, 0, 1);
      names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
      tags[0] = VT_EPEND;
      vids[0] = name_pend;
      names[1] = plan_pend_for_elem(pl, WK_SRC);
      tags[1] = VT_EPEND;
      vids[1] = ptr;
      rel =
          plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 1);
      plan_push_srcop(pl, rel, ptr, 0);
    }
  }

  /* -- facts: triple sugar and the general form (attr-set dedup) -- */
  for (i = 0; i < sub->fact_count; i++) {
    const SubFact *f = &sub->facts[i];
    double conf =
        f->has_confidence ? f->confidence : sub->intent[INTENT_CONVICTION];
    if (f->is_triple) {
      u32 names[2], vids[2], rp;
      u8 tags[2];
      names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
      snprintf(path, sizeof path, "facts[%u].s", i);
      plan_slot_value(pl, f->s, path, 1, &tags[0], &vids[0], 0);
      snprintf(path, sizeof path, "facts[%u].p", i);
      reject_cache_predicate(pl, f->p, path);
      names[1] = plan_ref(pl, f->p, path, NONE_U32, 0, 0);
      snprintf(path, sizeof path, "facts[%u].o", i);
      /* a `resolves` fact closes an existing question/task: its object must
       * already exist, else it silently mints a phantom (spec §8, MCP note). */
      plan_slot_value(pl, f->o, path, 1, &tags[1], &vids[1],
                      pl->pends[names[1]].existing == WK_RESOLVES);
      rp = plan_relation(pl, names, tags, vids, 2, f->status, conf, f->salience,
                         (u8)f->has_salience, 1);
      if (!span_absent(f->src)) {
        snprintf(path, sizeof path, "facts[%u].src", i);
        plan_push_srcop(pl, rp, plan_ref(pl, f->src, path, NONE_U32, 0, 1), 1);
      }
      plan_push_modalop(pl, rp, f->modal);
    } else {
      u32 keys[5], val_off[5], val_cnt[5], a, total = 1;
      PlanVal vals[5 * LEGEND_LIST_CAP];
      u32 vtotal = 0;
      u32 from_i = NONE_U32, to_i = NONE_U32, target_i = NONE_U32,
          property_i = NONE_U32;
      snprintf(path, sizeof path, "facts[%u]", i);
      for (a = 0; a < f->attr_count; a++) {
        const AttrEntry *ae = &sub->attr_pool[f->attr_start + a];
        u32 nlen = normalize_into_scratch(buf + ae->key.off, ae->key.len);
        if (nlen == 4 && memcmp(g_norm_buf, "from", 4) == 0)
          from_i = a;
        if (nlen == 2 && memcmp(g_norm_buf, "to", 2) == 0)
          to_i = a;
        if (nlen == 6 && memcmp(g_norm_buf, "target", 6) == 0)
          target_i = a;
        if (nlen == 8 && memcmp(g_norm_buf, "property", 8) == 0)
          property_i = a;
      }
      if (from_i != NONE_U32 && to_i != NONE_U32) {
        /* event-shaped: triggers supersession exactly as a change
         * does (spec §5); the event keeps exactly its submitted
         * slots. The cache needs a (target, property) key, and an
         * action that flips state never runs off a cross-product. */
        u32 names[5], vids[5];
        u8 tags[5];
        u32 event_idx, curname_pend;
        CurNamePeek cn;
        Prior prior;
        double eff = g->policy.supersession_threshold *
                     (1.0 - sub->intent[INTENT_PREDICTION_ERROR]);
        if (target_i == NONE_U32 || property_i == NONE_U32)
          fail(ERR_PARSE, path,
               "an event-shaped fact needs target and property slots");
        for (a = 0; a < f->attr_count; a++)
          if (sub->attr_pool[f->attr_start + a].val_count != 1)
            fail(ERR_PARSE, path, "event-shaped fact slots take single values");
        for (a = 0; a < f->attr_count; a++) {
          const AttrEntry *ae = &sub->attr_pool[f->attr_start + a];
          char vpath[224];
          int listable = a != property_i; /* the property value folds
              into the attribute vocabulary (spec §7) */
          snprintf(vpath, sizeof vpath, "facts[%u].attrs.%.*s", i,
                   (int)(ae->key.len > 64 ? 64 : ae->key.len),
                   buf + ae->key.off);
          reject_cache_predicate(pl, ae->key, vpath);
          keys[a] = plan_ref(pl, ae->key, vpath, NONE_U32, 0, 0);
          names[a] = keys[a];
          plan_slot_value(pl, sub->span_pool[ae->val_start], vpath, listable,
                          &tags[a], &vids[a], 0);
          if ((a == target_i || a == property_i || a == from_i || a == to_i) &&
              tags[a] != VT_EPEND)
            fail(ERR_PARSE, vpath, "expected an element ref");
        }
        event_idx =
            plan_relation(pl, names, tags, vids, (u8)f->attr_count, f->status,
                          conf, f->salience, (u8)f->has_salience, 1);
        cn = plan_curname_peek(pl, vids[property_i]);
        plan_prior_peek(pl, vids[target_i], cn, &prior);
        curname_pend = plan_curname_pend(pl, vids[property_i], path);
        plan_change_state(pl, event_idx, vids[target_i], vids[property_i],
                          curname_pend, vids[to_i], &prior, conf, conf >= eff);
        if (!span_absent(f->src)) {
          snprintf(path, sizeof path, "facts[%u].src", i);
          plan_push_srcop(pl, event_idx,
                          plan_ref(pl, f->src, path, NONE_U32, 0, 1), 1);
        }
        plan_push_modalop(pl, event_idx, f->modal);
        continue;
      }
      for (a = 0; a < f->attr_count; a++) {
        const AttrEntry *ae = &sub->attr_pool[f->attr_start + a];
        u32 v;
        char vpath[224];
        snprintf(vpath, sizeof vpath, "facts[%u].attrs.%.*s", i,
                 (int)(ae->key.len > 64 ? 64 : ae->key.len), buf + ae->key.off);
        reject_cache_predicate(pl, ae->key, vpath);
        keys[a] = plan_ref(pl, ae->key, vpath, NONE_U32, 0, 0);
        val_off[a] = vtotal;
        val_cnt[a] = ae->val_count;
        total *= ae->val_count;
        if (total > LEGEND_LIST_CAP)
          fail(ERR_LIMIT_EXCEEDED, path,
               "fact expands to more than %d relations", LEGEND_LIST_CAP);
        for (v = 0; v < ae->val_count; v++) {
          plan_slot_value(pl, sub->span_pool[ae->val_start + v], vpath, 1,
                          &vals[vtotal].tag, &vals[vtotal].id, 0);
          vtotal++;
        }
      }
      {
        u32 idx[5] = {0, 0, 0, 0, 0};
        u32 c;
        for (c = 0; c < total; c++) {
          u32 names[5], vids[5], rp;
          u8 tags[5];
          for (a = 0; a < f->attr_count; a++) {
            const PlanVal *pv = &vals[val_off[a] + idx[a]];
            names[a] = keys[a];
            tags[a] = pv->tag;
            vids[a] = pv->id;
          }
          rp =
              plan_relation(pl, names, tags, vids, (u8)f->attr_count, f->status,
                            conf, f->salience, (u8)f->has_salience, 1);
          if (!span_absent(f->src)) {
            snprintf(path, sizeof path, "facts[%u].src", i);
            plan_push_srcop(pl, rp, plan_ref(pl, f->src, path, NONE_U32, 0, 1),
                            1);
          }
          plan_push_modalop(pl, rp, f->modal);
          for (a = 0; a < f->attr_count; a++) {
            if (++idx[a] < val_cnt[a])
              break;
            idx[a] = 0;
          }
        }
      }
    }
  }

  /* -- changes: the supersession form (spec §5 expansion) -- */
  for (i = 0; i < sub->change_count; i++) {
    const SubChange *c = &sub->changes[i];
    double conf =
        c->has_confidence ? c->confidence : g->policy.default_confidence;
    double eff = g->policy.supersession_threshold *
                 (1.0 - sub->intent[INTENT_PREDICTION_ERROR]);
    int gate = c->intervened || conf >= eff; /* intervened bypasses (spec §5) */
    u32 target_pend, property_pend, curname_pend, to_pend;
    u32 from_pend = NONE_U32, event_pend = NONE_U32;
    u32 event_idx;
    CurNamePeek cn;
    Prior prior;
    snprintf(path, sizeof path, "changes[%u].target", i);
    /* NOTE: changes.target may legitimately mint (one-shot "set a property on a
     * new element" -- spec fixtures f05/f08 rely on it), so it is NOT must_exist;
     * the typo-phantom footgun is a UX concern for a future warning, not an error. */
    target_pend = plan_ref(pl, c->target, path, NONE_U32, 0, 1);
    snprintf(path, sizeof path, "changes[%u].property", i);
    property_pend = plan_ref(pl, c->property, path, NONE_U32, 0, 0);
    cn = plan_curname_peek(pl, property_pend);
    plan_prior_peek(pl, target_pend, cn, &prior);
    if (prior.found) {
      /* the store's actual prior is authoritative and already an element, so it
       * fills `from` — a caller-supplied `from` that disagrees is a mismatch we
       * must NOT mint into a phantom value-element (it just clutters the graph).
       * The filled value counts as a write-position touch (spec §5). */
      if (prior.staged)
        from_pend = prior.val_id;
      else if (prior.val_tag == TERM_ELEM)
        from_pend = plan_pend_for_elem(pl, prior.val_id);
      if (from_pend != NONE_U32)
        plan_touch(pl, from_pend, 1);
    } else if (!span_absent(c->from)) {
      /* no stored prior: the explicit `from` is the only record of the prior
       * value — legitimate history, reify it as given */
      snprintf(path, sizeof path, "changes[%u].from", i);
      from_pend = plan_ref(pl, c->from, path, NONE_U32, 0, 1);
    }
    snprintf(path, sizeof path, "changes[%u].to", i);
    to_pend = plan_ref(pl, c->to, path, NONE_U32, 0, 1);
    /* Prose backstop (trial R5 #149/#152): a changes.to that would MINT a new
     * element and reads as prose (a paragraph, not a short value) is the dominant
     * prose_name source. Reject at the mint — never on a resolve, where a long value
     * that names an existing element is a legitimate reference — so the model logs a
     * short canonical value and puts the detail in the target's summary. Shares the
     * audit's threshold; measures normalized length to match the name it would become. */
    if (pl->pends[to_pend].existing == NONE_U32) {
      u32 tolen = normalize_into_scratch(pl->buf + c->to.off, c->to.len);
      if (tolen > g_aud_name_chars)
        fail(ERR_PROSE_VALUE, path,
             "changes.to is %u chars -- it becomes an element NAME, so it must be a "
             "SHORT canonical value (e.g. \"built\", \"approved\"). Put the detail in "
             "the target's summary by resubmitting the element: "
             "elements:[{\"name\":..., \"summary\":...}]",
             tolen);
    }
    if (!span_absent(c->event)) {
      snprintf(path, sizeof path, "changes[%u].event", i);
      event_pend = plan_ref(pl, c->event, path, NONE_U32, 0, 1);
    }
    {
      u32 names[5], vids[5];
      u8 tags[5], n = 0;
      names[n] = plan_pend_for_elem(pl, WK_SUBJECT);
      tags[n] = VT_EPEND;
      vids[n++] = event_pend != NONE_U32 ? event_pend : target_pend;
      names[n] = plan_pend_for_elem(pl, WK_TARGET);
      tags[n] = VT_EPEND;
      vids[n++] = target_pend;
      names[n] = plan_pend_for_elem(pl, WK_PROPERTY);
      tags[n] = VT_EPEND;
      vids[n++] = property_pend;
      if (from_pend != NONE_U32) {
        names[n] = plan_pend_for_elem(pl, WK_FROM);
        tags[n] = VT_EPEND;
        vids[n++] = from_pend;
      }
      names[n] = plan_pend_for_elem(pl, WK_TO);
      tags[n] = VT_EPEND;
      vids[n++] = to_pend;
      event_idx =
          plan_relation(pl, names, tags, vids, n, ST_ASSERTED, conf, 0.0, 0, 1);
    }
    snprintf(path, sizeof path, "changes[%u]", i);
    curname_pend = plan_curname_pend(pl, property_pend, path);
    plan_change_state(pl, event_idx, target_pend, property_pend, curname_pend,
                      to_pend, &prior, conf, gate);
    if (!span_absent(c->src)) {
      snprintf(path, sizeof path, "changes[%u].src", i);
      plan_push_srcop(pl, event_idx, plan_ref(pl, c->src, path, NONE_U32, 0, 1),
                      1);
    }
  }

  /* -- retract: precise refs, cascade to derived caches (spec §5) -- */
  for (i = 0; i < sub->retract_count; i++) {
    const SubRetract *rt = &sub->retracts[i];
    u32 rid;
    snprintf(path, sizeof path, "retract[%u]", i);
    if (rt->is_rel_ref) {
      if (!span_parse_rel_id(buf, rt->rel_ref, &rid) ||
          rid >= g->relation_count)
        fail(ERR_UNKNOWN_REF, path, "unknown relation %.*s",
             (int)(rt->rel_ref.len > 32 ? 32 : rt->rel_ref.len),
             buf + rt->rel_ref.off);
    } else {
      rid = plan_resolve_fact_shape(pl, &rt->fact, path);
    }
    if (rid < WK_RELATION_COUNT)
      fail(ERR_PARSE, path, "cannot retract a core ontology relation");
    u32vec_push(&pl->retract_echo, rid);
    if (g->relations[rid].status != ST_RETRACTED)
      plan_push_flip(pl, 0, rid, ST_RETRACTED);
    {
      u32 link = g->meta_by_target[rid];
      while (link != NONE_U32) {
        u32 mid = g->rel_links[link].rel;
        link = g->rel_links[link].next;
        {
          const Relation *m = &g->relations[mid];
          u32 a, cache = NONE_U32;
          int is_deriv = 0;
          if (m->status == ST_RETRACTED)
            continue;
          for (a = 0; a < m->attr_count; a++) {
            if (m->attrs[a].name == WK_DERIVED_FROM &&
                m->attrs[a].value.tag == TERM_REL &&
                m->attrs[a].value.id == rid)
              is_deriv = 1;
            if (m->attrs[a].name == WK_SUBJECT &&
                m->attrs[a].value.tag == TERM_REL)
              cache = m->attrs[a].value.id;
          }
          if (!is_deriv || cache == NONE_U32)
            continue;
          if (g->relations[cache].status != ST_RETRACTED &&
              !plan_flip_staged_already(pl, 0, cache))
            plan_push_flip(pl, 0, cache, ST_RETRACTED);
          /* acknowledge the cascade even when it is a no-op —
           * re-retracting must echo what the first retract did */
          {
            u32 e, seen = 0;
            for (e = 0; e < pl->retract_echo.count; e++)
              if (pl->retract_echo.v[e] == cache) {
                seen = 1;
                break;
              }
            if (!seen)
              u32vec_push(&pl->retract_echo, cache);
          }
        }
      }
    }
  }

  /* -- merge: caller-confirmed folds; precise refs only (spec §8) -- */
  for (i = 0; i < sub->merge_count; i++) {
    const SubMerge *m = &sub->merges[i];
    MergeOp op;
    snprintf(path, sizeof path, "merge[%u].from", i);
    op.from_elem = plan_precise_elem(pl, m->from, path);
    if (is_core_vocab(pl->g, op.from_elem))
      fail(ERR_PARSE, path, "cannot merge away a core vocabulary element");
    snprintf(path, sizeof path, "merge[%u].into", i);
    op.into_elem = plan_precise_elem(pl, m->into, path);
    op.from_span = m->from;
    op.into_span = m->into;
    pl->mergeops = (MergeOp *)xgrow(pl->mergeops, pl->mergeop_count + 1,
                                    &pl->mergeop_cap, sizeof *pl->mergeops);
    pl->mergeops[pl->mergeop_count++] = op;
  }

  /* -- metas: instance_of typing, change metas (derived_from/supersedes),
   * src pointers, source provenance. These mint after every base relation
   * so the base ids stay contiguous in walk order (plan §3.17); none of
   * them list in the writes arrays. -- */
  for (i = 0; i < sub->element_count; i++) {
    u32 names[2], vids[2];
    u8 tags[2];
    if (pl->elem_entry_kind[i] == NONE_U32)
      continue;
    /* correcting an existing element's kind supersedes the old instance_of */
    plan_supersede_old_kind(pl, pl->pends[pl->elem_entry_pend[i]].existing,
                            pl->pends[pl->elem_entry_kind[i]].existing);
    names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
    tags[0] = VT_EPEND;
    vids[0] = pl->elem_entry_pend[i];
    names[1] = plan_pend_for_elem(pl, WK_INSTANCE_OF);
    tags[1] = VT_EPEND;
    vids[1] = pl->elem_entry_kind[i];
    plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 0);
  }
  for (i = 0; i < pl->derivmeta_count; i++) {
    u32 names[2], vids[2];
    u8 tags[2];
    names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
    plan_rel_slot(pl, &tags[0], &vids[0], pl->derivmetas[i].cache_idx);
    names[1] = plan_pend_for_elem(pl, WK_DERIVED_FROM);
    plan_rel_slot(pl, &tags[1], &vids[1], pl->derivmetas[i].event_idx);
    plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 0);
  }
  for (i = 0; i < pl->supmeta_count; i++) {
    u32 names[2], vids[2];
    u8 tags[2];
    names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
    plan_rel_slot(pl, &tags[0], &vids[0], pl->supmetas[i].cache_idx);
    names[1] = plan_pend_for_elem(pl, WK_SUPERSEDES);
    if (pl->supmetas[i].prior_staged)
      plan_rel_slot(pl, &tags[1], &vids[1], pl->supmetas[i].prior_id);
    else {
      tags[1] = VT_RCONST;
      vids[1] = pl->supmetas[i].prior_id;
    }
    plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 0);
  }
  for (i = 0; i < pl->srcop_count; i++) {
    u32 names[2], vids[2];
    u8 tags[2];
    names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
    tags[0] = VT_EPEND;
    vids[0] = pl->srcops[i].ptr_pend;
    names[1] = plan_pend_for_elem(pl, WK_INSTANCE_OF);
    tags[1] = VT_EPEND;
    vids[1] = plan_pend_for_elem(pl, WK_KIND_POINTER);
    plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 0);
    if (!pl->srcops[i].meta)
      continue; /* element form: the base relation is the link */
    names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
    plan_rel_slot(pl, &tags[0], &vids[0], pl->srcops[i].rel_idx);
    names[1] = plan_pend_for_elem(pl, WK_SRC);
    tags[1] = VT_EPEND;
    vids[1] = pl->srcops[i].ptr_pend;
    plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 0);
  }
  /* modal meta-relations: one [subject: <fact-rel>, <modal>: <modal>] per set
   * bit. The attr NAME carries the modality (the value is a self-marker); the
   * subject slot is a relation ref, so meta_by_target links the fact and recall
   * can read its modality (§16.3). */
  for (i = 0; i < pl->modalop_count; i++) {
    u32 bit;
    for (bit = MODF_INTERVENED; bit <= MODF_GENERAL; bit <<= 1) {
      u32 names[2], vids[2], me;
      u8 tags[2];
      if (!(pl->modalops[i].modal & bit))
        continue;
      me = plan_pend_for_elem(pl, modal_elem_id(pl->g, bit));
      names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
      plan_rel_slot(pl, &tags[0], &vids[0], pl->modalops[i].rel_idx);
      names[1] = me;
      tags[1] = VT_EPEND;
      vids[1] = me;
      plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 0);
    }
  }
  /* source is provenance FOR facts minted this call; with nothing to attach it
   * to it must not mint a lone element named after the provenance sentence (a
   * recurring phantom footgun). Only reify it once a listed relation exists. */
  if (!span_absent(sub->source)) {
    u32 r;
    int any_listed = 0;
    for (r = 0; r < pl->rel_count; r++)
      if (pl->rels[r].listed) { any_listed = 1; break; }
    if (any_listed)
      pl->source_pend = plan_ref(pl, sub->source, "source", NONE_U32, 0, 0);
    for (r = 0; any_listed && r < pl->rel_count; r++) {
      u32 names[2], vids[2];
      u8 tags[2];
      if (!pl->rels[r].listed)
        continue;
      names[0] = plan_pend_for_elem(pl, WK_SUBJECT);
      if (pl->rels[r].existing != NONE_U32) {
        tags[0] = VT_RCONST;
        vids[0] = pl->rels[r].existing;
      } else {
        tags[0] = VT_RPEND;
        vids[0] = r;
      }
      names[1] = plan_pend_for_elem(pl, WK_SOURCE);
      tags[1] = VT_EPEND;
      vids[1] = pl->source_pend;
      plan_relation(pl, names, tags, vids, 2, ST_ASSERTED, 1.0, 0.0, 0, 0);
    }
  }

  /* -- focus: read positions (spec §8) — resolve for the frame walk, never
   * mint, never list; a name that resolves nothing reports resolved:false
   * (search is a recall whose focus doesn't resolve, spec §4) -- */
  for (i = 0; i < sub->focus_count; i++) {
    Span s = sub->focus[i];
    u32 id, nlen;
    ResOp fop;
    memset(&fop, 0, sizeof fop);
    snprintf(path, sizeof path, "focus[%u]", i);
    if (span_is_rel_ref(buf, s))
      fail(ERR_PARSE, path, "expected an element ref, got a relation ref");
    if (span_parse_elem_id(buf, s, &id)) {
      if (id >= g->element_count)
        fail(ERR_UNKNOWN_REF, path, "unknown element #%u", id);
      fop.is_pend = 0;
      fop.resolved = 1;
      fop.id = follow_redirect(g, id);
    } else {
      nlen = normalize_into_scratch(buf + s.off, s.len);
      if (nlen == 0)
        fail(ERR_PARSE, path, "name is empty after normalization");
      {
        u32 pend = smap_get(&pl->pend_by_norm, &pl->scratch, g_norm_buf, nlen);
        if (pend != NONE_U32) {
          if (pl->pends[pend].homonym)
            plan_log_homonym(pl, path, s, pend);
          fop.is_pend = 1;
          fop.resolved = 1;
          fop.id = pend;
        } else {
          u32 count;
          u32 e = resolve_tier1_n(g, g_norm_buf, nlen, NONE_U32, &count);
          if (e == NONE_U32) {
            ResOp *op;
            double score = 0.0;
            u32 cand_count = 0, cand_start = pl->cands.count;
            u32 hit = tier2_focus_miss(g, buf + s.off, s.len, &pl->cands,
                                       &cand_count, &score, 0);
            pl->resops = (ResOp *)xgrow(pl->resops, pl->resop_count + 1,
                                        &pl->resop_cap, sizeof *pl->resops);
            op = &pl->resops[pl->resop_count++];
            snprintf(op->at, sizeof op->at, "%s", path);
            op->submitted = s;
            op->is_pend = 0;
            op->resolved = hit != NONE_U32;
            op->id = hit;
            op->via = VIA_LEXICAL;
            op->score = score;
            op->cand_start = cand_start;
            op->cand_count = cand_count;
            if (hit == NONE_U32)
              continue; /* unresolved: candidates only, no focus walk */
            e = hit;    /* slam-dunk: joins the walk like a tier-1 hit */
          }
          if (count > 1) {
            ResOp *op;
            pl->resops = (ResOp *)xgrow(pl->resops, pl->resop_count + 1,
                                        &pl->resop_cap, sizeof *pl->resops);
            op = &pl->resops[pl->resop_count++];
            snprintf(op->at, sizeof op->at, "%s", path);
            op->submitted = s;
            op->is_pend = 0;
            op->resolved = 1;
            op->id = e;
            op->via = VIA_HOMONYM;
            op->score = 1.0;
            op->cand_start = op->cand_count = 0;
          }
          fop.is_pend = 0;
          fop.resolved = 1;
          fop.id = e;
        }
      }
    }
    pl->focus_ops = (ResOp *)xgrow(pl->focus_ops, pl->focus_op_count + 1,
                                   &pl->focus_op_cap, sizeof *pl->focus_ops);
    pl->focus_ops[pl->focus_op_count++] = fop;
  }

  /* -- near_matches (spec §7): write-position mints scoring >= 0.6 against
   * a pre-existing element, first-touch order. Constructed names are not
   * caller positions and a new:true mint was explicitly forced (its tier-1
   * twin is exact); both are exempt. -- */
  for (i = 0; i < pl->pend_count; i++) {
    const PendElem *p = &pl->pends[i];
    double score;
    u32 existing;
    if (p->existing != NONE_U32 || p->raw != NONE_U32 || p->forced)
      continue;
    if (span_absent(p->surface))
      continue;
    existing = tier2_best_near(g, buf + p->surface.off, p->surface.len, &score);
    if (existing == NONE_U32)
      continue;
    pl->nears = (NearOp *)xgrow(pl->nears, pl->near_count + 1, &pl->near_cap,
                                sizeof *pl->nears);
    memcpy(pl->nears[pl->near_count].at, p->path,
           sizeof pl->nears[pl->near_count].at);
    pl->nears[pl->near_count].pend = i;
    pl->nears[pl->near_count].existing = existing;
    pl->nears[pl->near_count].score = score;
    pl->near_count++;
  }
}

/* ---- apply + report ---- */

typedef struct {
  u32 rel;
  u32 support_count;
  u8 promoted;
} ReusedRel;
typedef struct {
  char at[224];
  Span submitted;
  u8 resolved;
  u32 elem;
  u8 via;
  double score;
  u32 cand_start, cand_count; /* report cand pool */
} ResEntry;
typedef struct {
  char at[160];
  u32 minted, existing;
  double score;
} NearEntry;
typedef struct {
  u32 event_rel, prior_rel, property_elem;
  u8 pval_tag;
  u32 pval_id, to_val;
} ConflictEntry;
typedef struct {
  u32 elem, kind_elem;
  u32 keys[LEGEND_LIST_CAP];
  u32 count;
} DriftEntry;
typedef struct {
  Span from, into;
} MergeEcho;
typedef struct {
  u32 elem, rel;
} PairER;

/* The per-tick report the frame renders from: the writes accounting plus
 * resolution/conflicts/template_drift and the focus seeds. Recalls fill the
 * same struct with empty writes (plan §3.14). */
typedef struct {
  u32 tick;
  i64 at_secs;
  U32Vec minted_elems; /* element ids, first-touch order */
  U32Vec reused_elems;
  U32Vec minted_rels; /* base relation ids, walk order */
  ReusedRel *reused_rels;
  u32 reused_rel_count, reused_rel_cap;
  U32Vec retracted; /* echo order: submitted refs, then cascaded caches */
  MergeEcho *merges;
  u32 merge_count, merge_cap;
  ResEntry *res;
  u32 res_count, res_cap;
  ScoredVec cands; /* candidate pool behind resolved:false entries */
  NearEntry *nears;
  u32 near_count, near_cap;
  ConflictEntry *conflicts;
  u32 conflict_count, conflict_cap;
  DriftEntry *drifts;
  u32 drift_count, drift_cap;
  PairER *ptrs;
  u32 ptr_count, ptr_cap; /* pointer elem -> base rel, mint order */
  PairER *srcs;
  u32 src_count, src_cap; /* source elem -> minted base rel */
  U32Vec focus_elems;     /* resolved focus, payload order, deduped */
  const char *payload;    /* Span backing store for submitted/echo text */
  int has_focus;          /* frame form: full packet vs compact */
  int orientation; /* focus-less recall: overview + store-wide sections */
  int is_recall;   /* recall: the whole focus neighborhood is recency */
  double curiosity; /* recall intent (0..1): >0 biases recent toward novelty */
  const char *focus_query; /* joined raw focus text; ranks facts by relevance
                              on a focused recall (NULL = recency only) */
} WriteReport;

static void report_reset(WriteReport *w) {
  free(w->minted_elems.v);
  free(w->reused_elems.v);
  free(w->minted_rels.v);
  free(w->reused_rels);
  free(w->retracted.v);
  free(w->merges);
  free(w->res);
  free(w->cands.v);
  free(w->nears);
  free(w->conflicts);
  free(w->drifts);
  free(w->ptrs);
  free(w->srcs);
  free(w->focus_elems.v);
  memset(w, 0, sizeof *w);
}

static void report_push_res(WriteReport *w, const ResEntry *e) {
  w->res =
      (ResEntry *)xgrow(w->res, w->res_count + 1, &w->res_cap, sizeof *w->res);
  w->res[w->res_count++] = *e;
}

/* ---- merge fold (spec §8) ---- */

static double d_max(double a, double b) { return a > b ? a : b; }
static u32 u_max(u32 a, u32 b) { return a > b ? a : b; }

static void stats_max_merge(Stats *a, const Stats *b) {
  a->confidence = d_max(a->confidence, b->confidence);
  a->activation = d_max(a->activation, b->activation);
  a->salience = d_max(a->salience, b->salience);
  a->stability = d_max(a->stability, b->stability);
  a->access_count = u_max(a->access_count, b->access_count);
  a->focus_success_count =
      u_max(a->focus_success_count, b->focus_success_count);
  a->support_count = u_max(a->support_count, b->support_count);
  a->support_diversity = u_max(a->support_diversity, b->support_diversity);
  a->last_seen = u_max(a->last_seen, b->last_seen);
}

/* Fold `from` into `into`: names become aliases, every relation referencing
 * from is rewritten, live collisions collapse via attr-set dedup with stats
 * max-merged, and from's slot becomes a redirect tombstone. Tombstones that
 * pointed at from re-point at into, keeping every redirect single-hop (the
 * snapshot reader rejects chains). The caller rebuilds the derived indices
 * once after all folds. */
static void fold_element(Hypergraph *g, u32 from, u32 into) {
  Element *ef = &g->elements[from];
  Element *ei = &g->elements[into];
  static U32Vec rewritten;
  u32 i, k;
  rewritten.count = 0;
  for (i = 0; i < ef->names.count; i++) {
    u32 sid = ef->names.v[i];
    int present = 0;
    for (k = 0; k < ei->names.count; k++)
      if (ei->names.v[k] == sid)
        present = 1;
    if (!present)
      u32vec_push(&ei->names, sid);
  }
  if (ei->summary == NONE_U32)
    ei->summary = ef->summary;
  stats_max_merge(&ei->stats, &ef->stats);
  for (i = 0; i < g->relation_count; i++) {
    Relation *r = &g->relations[i];
    int hit = 0;
    for (k = 0; k < r->attr_count; k++) {
      if (r->attrs[k].name == from) {
        r->attrs[k].name = into;
        hit = 1;
      }
      if (r->attrs[k].value.tag == TERM_ELEM && r->attrs[k].value.id == from) {
        r->attrs[k].value.id = into;
        hit = 1;
      }
    }
    if (hit && r->status < ST_SUPERSEDED)
      u32vec_push(&rewritten, i);
  }
  ef->redirect = into;
  for (i = 0; i < g->element_count; i++)
    if (i != from && g->elements[i].redirect == from)
      g->elements[i].redirect = into;
  /* collision collapse: only live pairs fold (a superseded twin is history
   * and must not be revived); the lower id survives, the other is dropped
   * as Retracted — invisible everywhere and snapshot-representable */
  for (i = 0; i < rewritten.count; i++) {
    u32 rid = rewritten.v[i];
    u32 s;
    if (g->relations[rid].status >= ST_SUPERSEDED)
      continue;
    for (s = 0; s < g->relation_count; s++) {
      Relation *keep, *drop;
      u32 keep_id, drop_id;
      if (s == rid || g->relations[s].status >= ST_SUPERSEDED)
        continue;
      if (!attr_set_eq(g->relations[s].attrs, g->relations[s].attr_count,
                       g->relations[rid].attrs, g->relations[rid].attr_count))
        continue;
      keep_id = s < rid ? s : rid;
      drop_id = keep_id == s ? rid : s;
      keep = &g->relations[keep_id];
      drop = &g->relations[drop_id];
      if (drop->status < keep->status)
        keep->status = drop->status;
      stats_max_merge(&keep->stats, &drop->stats);
      for (k = 0; k < drop->supporters.count; k++) {
        u32 sup = drop->supporters.v[k], j;
        int present = 0;
        for (j = 0; j < keep->supporters.count; j++)
          if (keep->supporters.v[j] == sup)
            present = 1;
        if (!present)
          u32vec_push(&keep->supporters, sup);
      }
      if (keep->supporters.count)
        keep->stats.support_diversity = keep->supporters.count;
      drop->status = ST_RETRACTED;
      if (drop_id == rid)
        break;
    }
  }
}

/* Merge aftermath: a fold can leave two live current_* caches for the same
 * (into, current_<property>) key when the folded elements tracked the same
 * property to different values — attr-set dedup cannot collapse them (the
 * values differ), so both would show in `state` and one becomes a phantom.
 * Keep the newest (created_at, tie highest id) live and supersede the rest,
 * minting a supersedes meta so the loser reads as history. The caller rebuilds
 * indices after all folds. */
static void reconcile_merged_caches(Hypergraph *g, u32 into, u32 tick) {
  u32 i, j, n = g->relation_count;
  for (i = 0; i < n; i++) {
    Relation *ri = &g->relations[i];
    u32 a, iname = NONE_U32;
    int isubj = 0;
    if (ri->status >= ST_SUPERSEDED || ri->attr_count != 2)
      continue;
    for (a = 0; a < 2; a++) {
      if (ri->attrs[a].name == WK_SUBJECT &&
          ri->attrs[a].value.tag == TERM_ELEM && ri->attrs[a].value.id == into)
        isubj = 1;
      else if (elem_name_l(g, ri->attrs[a].name) > 8 &&
               memcmp(elem_name(g, ri->attrs[a].name), "current_", 8) == 0)
        iname = ri->attrs[a].name;
    }
    if (!isubj || iname == NONE_U32)
      continue;
    /* a strictly newer/higher live peer on the same key makes i the loser */
    for (j = 0; j < n; j++) {
      Relation *rj = &g->relations[j];
      int jsubj = 0, jsame = 0;
      u32 b;
      if (j == i || rj->status >= ST_SUPERSEDED || rj->attr_count != 2)
        continue;
      for (b = 0; b < 2; b++) {
        if (rj->attrs[b].name == WK_SUBJECT &&
            rj->attrs[b].value.tag == TERM_ELEM &&
            rj->attrs[b].value.id == into)
          jsubj = 1;
        else if (rj->attrs[b].name == iname)
          jsame = 1;
      }
      if (!jsubj || !jsame)
        continue;
      if (rj->created_at > ri->created_at ||
          (rj->created_at == ri->created_at && j > i)) {
        Attr meta[2];
        meta[0].name = WK_SUBJECT;
        meta[0].value.tag = TERM_REL;
        meta[0].value.id = j;
        meta[1].name = WK_SUPERSEDES;
        meta[1].value.tag = TERM_REL;
        meta[1].value.id = i;
        mint_relation(g, meta, 2, ST_ASSERTED, 1.0, 0.0, tick, NONE_U32);
        g->relations[i].status = ST_SUPERSEDED;
        break;
      }
    }
  }
}

/* Apply scratch, static so a trapped fail() (store_full) leaves it reachable.
 */
static u32 *g_pend_real, g_pend_real_cap;
static u32 *g_rel_real, g_rel_real_cap;
static u8 *g_rel_promoted; /* per plan-rel: Defeasible->Asserted this tick */
static u32 g_rel_promoted_cap;
static U32Vec g_reinforced; /* this tick's touched relations: the decay seeds */

/* PASS 2: execute the staged plan. This is the ONLY function that mutates the
 * graph, and (store_full aside) it cannot fail — plan already proved every ref
 * good. It advances the clock, then walks the ops in the fixed apply order
 * (elements -> relations -> effect-topology -> flips -> caches -> merges ->
 * dynamics), building two index maps as it goes: g_pend_real[pend] = the real
 * element id that pend became, and g_rel_real[plan-rel] = the real relation id.
 * Every staged op that referenced a pend or plan-rel is resolved through those
 * maps here. Mints append to the report's minted_* lists; reuses bump stats and
 * may trip promotion (Defeasible -> Asserted). Dynamics (touch/decay) run last
 * so the whole tick's activity is visible to the decay walk. */
static void apply_plan(Hypergraph *g, Plan *pl, WriteReport *out) {
  const Submission *sub = pl->sub;
  u32 tick, i;
  out->at_secs = now_unix_seconds();
  /* saves always tick: the reader rejects observe on save */
  if (g->clock >= g_cap_ticks)
    fail(ERR_STORE_FULL, NULL, "tick clock is full (%u ticks)", g->clock);
  g->stamps =
      (u64 *)xgrow(g->stamps, g->clock + 1, &g->stamp_cap, sizeof *g->stamps);
  g->stamps[g->clock] = (u64)out->at_secs;
  g->clock++;
  tick = g->clock;
  out->tick = tick;

  /* elements first: mints in pend (= first-touch) order */
  g_pend_real = (u32 *)xgrow(g_pend_real, pl->pend_count ? pl->pend_count : 1,
                             &g_pend_real_cap, sizeof *g_pend_real);
  for (i = 0; i < pl->pend_count; i++) {
    PendElem *p = &pl->pends[i];
    if (p->existing != NONE_U32) {
      g_pend_real[i] = p->existing;
      if (p->touched) {
        Element *e = &g->elements[p->existing];
        if (dyn_touch(&e->stats, e->created_at, tick, &g->policy))
          e->stats.access_count++;
        e->stats.last_seen = tick;
      }
      continue;
    }
    if (p->raw != NONE_U32)
      g_pend_real[i] = mint_element(
          g, str_ptr(&pl->raw, p->raw), str_len(&pl->raw, p->raw),
          g->policy.default_confidence, g->policy.salience_novel, tick);
    else
      g_pend_real[i] = mint_element(g, pl->buf + p->surface.off, p->surface.len,
                                    g->policy.default_confidence,
                                    g->policy.salience_novel, tick);
    /* the mint is the first touch (spec §3.1): activation lifts off zero */
    g->elements[g_pend_real[i]].stats.activation =
        hebb_bump(0.0, g->policy.hebbian_rate);
    u32vec_push(&out->minted_elems, g_pend_real[i]);
  }
  for (i = 0; i < pl->reused_order.count; i++)
    u32vec_push(&out->reused_elems, g_pend_real[pl->reused_order.v[i]]);

  /* explicit-entry extras: aliases extend the name list, summary latest-
   * write-wins, salience seeds importance (spec §5) */
  for (i = 0; i < sub->element_count; i++) {
    const SubElement *se = &sub->elements[i];
    Element *e = &g->elements[g_pend_real[pl->elem_entry_pend[i]]];
    u32 a;
    for (a = 0; a < se->alias_count; a++) {
      Span al = sub->span_pool[se->alias_start + a];
      u32 sid = graph_intern(g, pl->buf + al.off, al.len);
      u32 k, present = 0;
      for (k = 0; k < e->names.count; k++)
        if (e->names.v[k] == sid) {
          present = 1;
          break;
        }
      if (!present) {
        u32vec_push(&e->names, sid);
        index_norm_name(g, g_pend_real[pl->elem_entry_pend[i]],
                        pl->buf + al.off, al.len);
      }
    }
    if (!span_absent(se->summary))
      e->summary = graph_intern(g, pl->buf + se->summary.off, se->summary.len);
    if (se->has_salience)
      e->stats.salience = se->salience;
    if (se->has_confidence)
      e->stats.confidence = se->confidence;
    /* rename_to (pin 23): the new name becomes canonical (prepended and
     * indexed), the old canonical stays behind as an alias */
    if (!span_absent(se->rename_to)) {
      u32 sid = graph_intern(g, pl->buf + se->rename_to.off, se->rename_to.len);
      u32 k, at = NONE_U32;
      for (k = 0; k < e->names.count; k++)
        if (e->names.v[k] == sid)
          at = k;
      if (at == NONE_U32) {
        u32vec_push(&e->names, sid);
        at = e->names.count - 1;
        index_norm_name(g, g_pend_real[pl->elem_entry_pend[i]],
                        pl->buf + se->rename_to.off, se->rename_to.len);
      }
      if (at != 0) {
        memmove(e->names.v + 1, e->names.v, (size_t)at * sizeof *e->names.v);
        e->names.v[0] = sid;
      }
    }
  }
  for (i = 0; i < sub->template_count; i++) {
    const SubTemplate *st = &sub->templates[i];
    if (!span_absent(st->summary))
      g->elements[g_pend_real[pl->tmpl_entry_pend[i]]].summary =
          graph_intern(g, pl->buf + st->summary.off, st->summary.len);
  }

  /* relations in staged order: base rels first, then the metas (plan §3.17
   * keeps the reported ids contiguous in walk order) */
  g_rel_real = (u32 *)xgrow(g_rel_real, pl->rel_count ? pl->rel_count : 1,
                            &g_rel_real_cap, sizeof *g_rel_real);
  g_rel_promoted =
      (u8 *)xgrow(g_rel_promoted, pl->rel_count ? pl->rel_count : 1,
                  &g_rel_promoted_cap, sizeof *g_rel_promoted);
  for (i = 0; i < pl->rel_count; i++) {
    PlanRel *pr = &pl->rels[i];
    u32 source_elem = pl->source_pend != NONE_U32 && pr->listed
                          ? g_pend_real[pl->source_pend]
                          : NONE_U32;
    g_rel_promoted[i] = 0;
    if (pr->existing != NONE_U32) {
      Relation *r = &g->relations[pr->existing];
      g_rel_real[i] = pr->existing;
      if (dyn_touch(&r->stats, r->created_at, tick, &g->policy)) {
        dyn_arousal_salience(&r->stats, &g->policy,
                             sub->intent[INTENT_AROUSAL]);
        r->stats.access_count++;
      }
      r->stats.support_count += 1 + pr->extra_bumps;
      r->stats.last_seen = tick;
      r->stats.salience =
          clamp_unit(r->stats.salience + g->policy.salience_reuse_bump);
      if (source_elem != NONE_U32) {
        u32 k, present = 0;
        for (k = 0; k < r->supporters.count; k++)
          if (r->supporters.v[k] == source_elem) {
            present = 1;
            break;
          }
        if (!present) {
          u32vec_push(&r->supporters, source_elem);
          r->stats.support_diversity++;
        }
      }
      /* promotion (spec §8): repetition plus independence flips a
       * Defeasible claim to Asserted */
      if (r->status == ST_DEFEASIBLE &&
          r->stats.support_count >= g->policy.promotion_min_support &&
          r->stats.support_diversity >= g->policy.promotion_min_diversity) {
        r->status = ST_ASSERTED;
        g_rel_promoted[i] = 1;
      }
      continue;
    }
    {
      Attr attrs[5];
      u32 a;
      for (a = 0; a < pr->count; a++) {
        attrs[a].name = g_pend_real[pr->name_pend[a]];
        if (pr->vtag[a] == VT_EPEND) {
          attrs[a].value.tag = TERM_ELEM;
          attrs[a].value.id = g_pend_real[pr->vid[a]];
        } else if (pr->vtag[a] == VT_RCONST) {
          attrs[a].value.tag = TERM_REL;
          attrs[a].value.id = pr->vid[a];
        } else {
          attrs[a].value.tag = TERM_REL;
          attrs[a].value.id = g_rel_real[pr->vid[a]];
        }
      }
      g_rel_real[i] = mint_relation(
          g, attrs, pr->count, pr->status, pr->confidence,
          pr->has_salience ? pr->salience : g->policy.salience_novel, tick,
          source_elem);
      /* mint = first touch: activation lifts off zero (spec §3.1) */
      g->relations[g_rel_real[i]].stats.activation =
          hebb_bump(0.0, g->policy.hebbian_rate);
      g->relations[g_rel_real[i]].stats.support_count += pr->extra_bumps;
      if (pr->listed)
        u32vec_push(&out->minted_rels, g_rel_real[i]);
    }
  }
  for (i = 0; i < pl->reused_rel_order.count; i++) {
    u32 pi = pl->reused_rel_order.v[i];
    u32 rid = pl->rels[pi].existing;
    out->reused_rels =
        (ReusedRel *)xgrow(out->reused_rels, out->reused_rel_count + 1,
                           &out->reused_rel_cap, sizeof *out->reused_rels);
    out->reused_rels[out->reused_rel_count].rel = rid;
    out->reused_rels[out->reused_rel_count].support_count =
        g->relations[rid].stats.support_count;
    out->reused_rels[out->reused_rel_count].promoted = g_rel_promoted[pi];
    out->reused_rel_count++;
  }

  /* importance effect-topology (spec §3.1, v1's CPEB tagging): a caller
   * salience above the threshold adds a scoped stability bump to relations
   * minted this tick — a salience-carrying fact bumps its own relation, a
   * salience-carrying element entry bumps every same-tick mint whose slots
   * touch that element. Never a global splash: reused and prior-tick
   * relations are untouched. */
  for (i = 0; i < pl->rel_count; i++) {
    const PlanRel *pr = &pl->rels[i];
    Relation *r;
    double boost = 0.0;
    u32 k, a;
    if (pr->existing != NONE_U32)
      continue;
    if (pr->has_salience && pr->salience > boost)
      boost = pr->salience;
    r = &g->relations[g_rel_real[i]];
    for (k = 0; k < sub->element_count; k++) {
      u32 carrier;
      if (!sub->elements[k].has_salience || sub->elements[k].salience <= boost)
        continue;
      carrier = g_pend_real[pl->elem_entry_pend[k]];
      for (a = 0; a < r->attr_count; a++)
        if (r->attrs[a].value.tag == TERM_ELEM &&
            r->attrs[a].value.id == carrier)
          boost = sub->elements[k].salience;
    }
    if (boost > DYN_CPEB_THRESHOLD)
      r->stats.stability = soft_cap_stability(
          r->stats.stability + DYN_CPEB_BOOST * boost, g->policy.stability_cap);
  }

  /* status flips: supersession then retraction, staged ids resolved */
  for (i = 0; i < pl->flip_count; i++) {
    u32 rid =
        pl->flips[i].staged ? g_rel_real[pl->flips[i].id] : pl->flips[i].id;
    Relation *r = &g->relations[rid];
    if (pl->flips[i].status == ST_SUPERSEDED) {
      if (r->status < ST_SUPERSEDED)
        r->status = ST_SUPERSEDED;
    } else if (r->status != ST_RETRACTED) {
      r->status = ST_RETRACTED;
    }
  }

  /* graph-PE salience seeds (plan §3.5): a conflict/supersession seed is an
   * override — it replaces whatever the touch-time bumps left behind */
  for (i = 0; i < pl->sal_count; i++) {
    u32 rid = pl->sals[i].staged ? g_rel_real[pl->sals[i].id] : pl->sals[i].id;
    g->relations[rid].stats.salience = pl->sals[i].value;
  }

  /* focus: read reinforcement + the frame's seed list. Reinforcement marks
   * the paths for future ranking (spec §6): the focus element and its live
   * one-hop relations take a dynamics touch and gain focus_success_count,
   * which pin §3.4's related ranking and the orientation `active` list read. */
  g_reinforced.count = 0;
  for (i = 0; i < pl->rel_count; i++)
    u32vec_push_unique(&g_reinforced, g_rel_real[i]);
  for (i = 0; i < pl->focus_op_count; i++) {
    const ResOp *fop = &pl->focus_ops[i];
    u32 e, k, present = 0;
    if (!fop->resolved)
      continue;
    e = fop->is_pend ? g_pend_real[fop->id] : fop->id;
    for (k = 0; k < out->focus_elems.count; k++)
      if (out->focus_elems.v[k] == e)
        present = 1;
    if (present)
      continue;
    u32vec_push(&out->focus_elems, e);
    dyn_reinforce_focus(g, e, tick, sub->intent[INTENT_AROUSAL], &g_reinforced);
  }

  /* merge folds last, then one index rebuild covering all of them */
  {
    int folded = 0;
    for (i = 0; i < pl->mergeop_count; i++) {
      u32 from = follow_redirect(g, pl->mergeops[i].from_elem);
      u32 into = follow_redirect(g, pl->mergeops[i].into_elem);
      out->merges = (MergeEcho *)xgrow(out->merges, out->merge_count + 1,
                                       &out->merge_cap, sizeof *out->merges);
      out->merges[out->merge_count].from = pl->mergeops[i].from_span;
      out->merges[out->merge_count].into = pl->mergeops[i].into_span;
      out->merge_count++;
      if (from == into)
        continue; /* idempotent no-op (spec §9) */
      fold_element(g, from, into);
      reconcile_merged_caches(g, into, tick);
      folded = 1;
    }
    if (folded)
      rebuild_indices(g);
    /* a same-payload fold can drop a just-minted relation as Retracted via
     * the collision collapse; such an id is not a real write (finding 3) */
    if (folded) {
      u32 w = 0;
      for (i = 0; i < out->minted_rels.count; i++)
        if (g->relations[out->minted_rels.v[i]].status != ST_RETRACTED)
          out->minted_rels.v[w++] = out->minted_rels.v[i];
      out->minted_rels.count = w;
    }
  }

  /* utility-modulated decay over the focus radius (spec §3.1): the last
   * substrate mutation of the tick, seeded by everything reinforced above */
  dyn_decay_walk(g, &g_reinforced, tick, &g->policy);

  /* report conversions: staged idxs and pends become real ids */
  for (i = 0; i < pl->retract_echo.count; i++)
    u32vec_push(&out->retracted, pl->retract_echo.v[i]);
  for (i = 0; i < pl->resop_count; i++) {
    ResEntry e;
    u32 c;
    memcpy(e.at, pl->resops[i].at, sizeof e.at);
    e.submitted = pl->resops[i].submitted;
    e.resolved = pl->resops[i].resolved;
    e.elem = !pl->resops[i].resolved ? NONE_U32
             : pl->resops[i].is_pend ? g_pend_real[pl->resops[i].id]
                                     : pl->resops[i].id;
    e.via = pl->resops[i].via;
    e.score = pl->resops[i].score;
    e.cand_start = out->cands.count;
    for (c = 0; c < pl->resops[i].cand_count; c++)
      scored_push(&out->cands, pl->cands.v[pl->resops[i].cand_start + c].elem,
                  pl->cands.v[pl->resops[i].cand_start + c].score);
    e.cand_count = pl->resops[i].cand_count;
    report_push_res(out, &e);
  }
  for (i = 0; i < pl->near_count; i++) {
    out->nears = (NearEntry *)xgrow(out->nears, out->near_count + 1,
                                    &out->near_cap, sizeof *out->nears);
    memcpy(out->nears[out->near_count].at, pl->nears[i].at,
           sizeof out->nears[out->near_count].at);
    out->nears[out->near_count].minted = g_pend_real[pl->nears[i].pend];
    out->nears[out->near_count].existing = pl->nears[i].existing;
    out->nears[out->near_count].score = pl->nears[i].score;
    out->near_count++;
  }
  for (i = 0; i < pl->conflict_count; i++) {
    const ConflictOp *op = &pl->conflicts[i];
    ConflictEntry e;
    e.event_rel = g_rel_real[op->event_idx];
    e.prior_rel = op->prior_staged ? g_rel_real[op->prior_id] : op->prior_id;
    if (op->pval_tag == 2) {
      e.pval_tag = 0;
      e.pval_id = g_pend_real[op->pval_id];
    } else {
      e.pval_tag = op->pval_tag;
      e.pval_id = op->pval_id;
    }
    e.property_elem = g_pend_real[op->property_pend];
    e.to_val = g_pend_real[op->to_pend];
    out->conflicts =
        (ConflictEntry *)xgrow(out->conflicts, out->conflict_count + 1,
                               &out->conflict_cap, sizeof *out->conflicts);
    out->conflicts[out->conflict_count++] = e;
  }
  for (i = 0; i < pl->drift_count; i++) {
    const DriftOp *op = &pl->drifts[i];
    DriftEntry e;
    u32 k;
    e.elem = g_pend_real[op->inst_pend];
    e.kind_elem = g_pend_real[op->kind_pend];
    e.count = op->key_count;
    for (k = 0; k < op->key_count; k++)
      e.keys[k] = g_pend_real[op->keys[k]];
    out->drifts = (DriftEntry *)xgrow(out->drifts, out->drift_count + 1,
                                      &out->drift_cap, sizeof *out->drifts);
    out->drifts[out->drift_count++] = e;
  }
  /* pointers/sources aggregate over the minted base relations (M1 pin) */
  for (i = 0; i < pl->srcop_count; i++) {
    if (pl->rels[pl->srcops[i].rel_idx].existing != NONE_U32)
      continue;
    out->ptrs = (PairER *)xgrow(out->ptrs, out->ptr_count + 1, &out->ptr_cap,
                                sizeof *out->ptrs);
    out->ptrs[out->ptr_count].elem = g_pend_real[pl->srcops[i].ptr_pend];
    out->ptrs[out->ptr_count].rel = g_rel_real[pl->srcops[i].rel_idx];
    out->ptr_count++;
  }
  if (pl->source_pend != NONE_U32) {
    for (i = 0; i < pl->rel_count; i++) {
      if (!pl->rels[i].listed || pl->rels[i].existing != NONE_U32)
        continue;
      out->srcs = (PairER *)xgrow(out->srcs, out->src_count + 1, &out->src_cap,
                                  sizeof *out->srcs);
      out->srcs[out->src_count].elem = g_pend_real[pl->source_pend];
      out->srcs[out->src_count].rel = g_rel_real[i];
      out->src_count++;
    }
  }
}

/* One save tick over the substrate (no I/O): plan against the immutable
 * graph, then apply. Any plan failure leaves the graph and clock untouched. */
static void tick_save(Hypergraph *g, const Submission *sub,
                      const char *payload_buf, WriteReport *out) {
  static Plan pl; /* static: reachable when a trapped fail() longjmps out */
  plan_reset(&pl);
  report_reset(out);
  out->payload = payload_buf;
  out->has_focus = sub->focus_count > 0;
  plan_submission(&pl, g, sub, payload_buf);
  apply_plan(g, &pl, out);
}

/* One recall tick: resolve focus (read positions, spec §8), then advance the
 * clock and reinforce — retrieval is itself a memory event unless observe
 * (spec §3.1). Resolution completes before any mutation (spec §9). */
static void tick_recall(Hypergraph *g, const Recall *rec,
                        const char *payload_buf, WriteReport *out) {
  char path[64];
  u32 i;
  report_reset(out);
  out->payload = payload_buf;
  out->has_focus = 1;
  out->orientation = rec->focus_count == 0;
  out->is_recall = 1;
  out->curiosity = rec->intent[INTENT_CURIOSITY];
  for (i = 0; i < rec->focus_count; i++) {
    Span s = rec->focus[i];
    u32 id, nlen;
    snprintf(path, sizeof path, "focus[%u]", i);
    if (span_is_rel_ref(payload_buf, s))
      fail(ERR_PARSE, path, "expected an element ref, got a relation ref");
    if (span_parse_elem_id(payload_buf, s, &id)) {
      if (id >= g->element_count)
        fail(ERR_UNKNOWN_REF, path, "unknown element #%u", id);
      id = follow_redirect(g, id);
    } else {
      u32 count;
      nlen = normalize_into_scratch(payload_buf + s.off, s.len);
      if (nlen == 0)
        fail(ERR_PARSE, path, "name is empty after normalization");
      id = resolve_tier1_n(g, g_norm_buf, nlen, NONE_U32, &count);
      if (id == NONE_U32) {
        ResEntry e;
        double score = 0.0;
        u32 cand_count = 0, cand_start = out->cands.count;
        u32 hit = tier2_focus_miss(g, payload_buf + s.off, s.len, &out->cands,
                                   &cand_count, &score, rec->observe);
        memset(&e, 0, sizeof e);
        snprintf(e.at, sizeof e.at, "%s", path);
        e.submitted = s;
        e.resolved = hit != NONE_U32;
        e.elem = hit;
        e.via = VIA_LEXICAL;
        e.score = score;
        e.cand_start = cand_start;
        e.cand_count = cand_count;
        report_push_res(out, &e);
        if (hit != NONE_U32)
          u32vec_push_unique(&out->focus_elems, hit);
        continue;
      }
      if (count > 1) {
        ResEntry e;
        memset(&e, 0, sizeof e);
        snprintf(e.at, sizeof e.at, "%s", path);
        e.submitted = s;
        e.resolved = 1;
        e.elem = id;
        e.via = VIA_HOMONYM;
        e.score = 1.0;
        report_push_res(out, &e);
      }
    }
    u32vec_push_unique(&out->focus_elems, id);
  }
  /* The query that ranks the neighborhood by relevance (frame side), not by
   * recency: the explicit `query` field if given, else the joined focus terms.
   * The query drives F1 ranking ONLY — it is never resolved to an element, so
   * passing pure intent here cannot trigger the tier-2 focus-miss scan the way
   * an unresolvable focus term does. */
  {
    static char q[1024];
    u32 qn = 0;
    if (rec->query.len) {
      u32 cp = rec->query.len;
      if (cp > sizeof q - 1)
        cp = (u32)(sizeof q - 1);
      memcpy(q, payload_buf + rec->query.off, cp);
      qn = cp;
    } else {
      for (i = 0; i < rec->focus_count; i++) {
        Span s = rec->focus[i];
        u32 cp = s.len;
        if (qn && qn + 1 < sizeof q)
          q[qn++] = ' ';
        if (cp > sizeof q - 1 - qn)
          cp = (u32)(sizeof q - 1 - qn);
        memcpy(q + qn, payload_buf + s.off, cp);
        qn += cp;
      }
    }
    q[qn] = 0;
    out->focus_query = qn ? q : NULL;
  }
  out->at_secs = now_unix_seconds();
  if (!rec->observe) {
    if (g->clock >= g_cap_ticks)
      fail(ERR_STORE_FULL, NULL, "tick clock is full (%u ticks)", g->clock);
    g->stamps =
        (u64 *)xgrow(g->stamps, g->clock + 1, &g->stamp_cap, sizeof *g->stamps);
    g->stamps[g->clock] = (u64)out->at_secs;
    g->clock++;
    g_reinforced.count = 0;
    for (i = 0; i < out->focus_elems.count; i++)
      dyn_reinforce_focus(g, out->focus_elems.v[i], g->clock,
                          rec->intent[INTENT_AROUSAL], &g_reinforced);
    dyn_decay_walk(g, &g_reinforced, g->clock, &g->policy);
  }
  out->tick = g->clock;
}

/* ======================= S8 — frame (the packet) =========================
 *
 * Turn the finished tick into the ORIENTATION PACKET — the JSON frame the
 * caller reads (spec §7). This section is all output: it walks the graph and
 * the WriteReport and streams typed sections straight to stdout. No frame tree
 * is ever built in memory.
 *
 * Three frame shapes come out of one code path (print_frame):
 *   - FULL frame (recall with focus, or save with focus): every section —
 *     focus header, `state` (current values), the kind sections (decisions/
 *     constraints/open + any template kind), recent/history/related, pointers/
 *     sources.
 *   - COMPACT frame (a save with no focus): only the write-feedback fields
 *     (tick/at/store/resolution/writes/near_matches/conflicts/template_drift).
 *     Bulk writers don't pay to retrieve what they didn't ask for.
 *   - ORIENTATION frame (a recall with no focus): the session-boot view —
 *     an `overview` object replaces `focus`, and constraints/open/recent are
 *     scoped store-wide instead of to a neighborhood.
 *
 * DETERMINISM rules enforced here (so C and Rust emit byte-identical frames):
 *   - Numbers: only score/confidence are floats, both in [0,1]; fmt_unit_float
 *     prints them with integer arithmetic (the "centi-unit" trick below). No
 *     %f, ever.
 *   - Ordering: sections sort by (date desc, id desc); candidate lists by
 *     (score desc, id asc). Never by hash-table iteration.
 *   - Empty sections emit as [] (or {}), never omitted — one rule, differ-
 *     friendly.
 *   - Dates and timestamps are hand-formatted (no libc localtime), tz-free.
 */

/* The §3.7 number formatter: score/confidence quantized to q = x*100 + 0.5
 * and printed with pure integer code — never libc float formatting. Byte-
 * identical in C and Rust. Total over all doubles: values outside [0,1]
 * clamp to the nearest endpoint before the cast (a double-to-u32 conversion
 * out of range is undefined behavior, and stats loaded from a snapshot are
 * only checked for finiteness, not range). NaN prints "0". */
LEGEND_UNUSED static u32 fmt_unit_float(double x, char out[8]) {
  double scaled = x * 100.0 + 0.5;
  u32 q = scaled >= 100.0 ? 100 : scaled > 0.0 ? (u32)scaled : 0;
  u32 n = 0;
  if (q == 0) {
    out[0] = '0';
    out[1] = 0;
    return 1;
  }
  if (q >= 100) {
    out[0] = '1';
    out[1] = 0;
    return 1;
  }
  out[n++] = '0';
  out[n++] = '.';
  if (q % 10 == 0) {
    out[n++] = (char)('0' + q / 10); /* q=90 -> "0.9" */
  } else {
    out[n++] = (char)('0' + q / 10); /* q=5 -> "0.05" (zero-padded) */
    out[n++] = (char)('0' + q % 10);
  }
  out[n] = 0;
  return n;
}

/* Inverse of days_from_civil (Howard Hinnant's civil_from_days). */
static void civil_from_days(i64 z, i64 *yy, u32 *mm, u32 *dd) {
  i64 era, y;
  u32 doe, yoe, doy, mp;
  z += 719468;
  era = (z >= 0 ? z : z - 146096) / 146097;
  doe = (u32)(z - era * 146097);
  yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  y = (i64)yoe + era * 400;
  doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  mp = (5 * doy + 2) / 153;
  *dd = doy - (153 * mp + 2) / 5 + 1;
  *mm = mp < 10 ? mp + 3 : mp - 9;
  *yy = y + (*mm <= 2);
}

/* "YYYY-MM-DDTHH:MM:SSZ" from unix seconds; hand-rolled like parse_iso_date
 * so no libc time conversion touches output (deterministic, tz-free). The
 * buffer allows extreme LEGEND_NOW years without truncation. */
static void format_iso_utc(i64 secs, char out[40]) {
  i64 days = secs / 86400, rem = secs % 86400, yy;
  u32 mm, dd;
  if (rem < 0) {
    rem += 86400;
    days--;
  }
  civil_from_days(days, &yy, &mm, &dd);
  snprintf(out, 40, "%04lld-%02u-%02uT%02lld:%02lld:%02lldZ", (long long)yy, mm,
           dd, (long long)(rem / 3600), (long long)(rem / 60 % 60),
           (long long)(rem % 60));
}

static void frame_put_elem_entry(const Hypergraph *g, u32 id, int first) {
  if (!first)
    fputc(',', stdout);
  printf("{\"ref\":\"#%u\",\"name\":\"", id);
  json_escape_fputs(str_ptr(&g->strs, g->elements[id].names.v[0]), stdout);
  fputs("\"}", stdout);
}

/* Escaped output of a non-NUL-terminated byte range (payload spans). */
static void json_escape_fwrite(const char *s, u32 len, FILE *f) {
  u32 i;
  for (i = 0; i < len; i++) {
    unsigned char c = (unsigned char)s[i];
    if (c == '"' || c == '\\') {
      fputc('\\', f);
      fputc(c, f);
    } else if (c == '\n')
      fputs("\\n", f);
    else if (c == '\r')
      fputs("\\r", f);
    else if (c == '\t')
      fputs("\\t", f);
    else if (c < 0x20)
      fprintf(f, "\\u%04x", c);
    else
      fputc(c, f);
  }
}

static const char *const ST_NAMES[] = {"asserted", "entailed", "defeasible",
                                       "superseded", "retracted"};

/* The UTC day a tick landed on; tick 0 (the seed) predates the stamp table
 * and reads as day 0 (1970-01-01). */
static i64 frame_tick_day(const Hypergraph *g, u32 tick) {
  i64 secs = (tick >= 1 && tick <= g->clock) ? (i64)g->stamps[tick - 1] : 0;
  i64 days = secs / 86400;
  if (secs % 86400 < 0)
    days--;
  return days;
}

static void frame_put_day(const Hypergraph *g, u32 tick) {
  i64 yy;
  u32 mm, dd;
  civil_from_days(frame_tick_day(g, tick), &yy, &mm, &dd);
  printf("%04lld-%02u-%02u", (long long)yy, mm, dd);
}

static u32 rel_modal_count(const Hypergraph *g, u32 rid);
static void frame_put_modal_values(const Hypergraph *g, u32 rid);

/* A statement can be the CONTENT of another statement -- `Nick asked me [to
 * remove em dashes from prose]` -- and that nesting is what a chain of arrows
 * actually means: the inner statement is an ARGUMENT of the outer one, and the
 * shared node (`me`) pivots between them. Written as two sibling facts instead,
 * the pivot and the containment are both lost: `{Nick asked me}` no longer says
 * WHAT was asked, and `{me removes em dash}` asserts as standing truth what was
 * only requested.
 *
 * The store has been able to hold this since TERM_REL existed. The frame could
 * not SHOW it -- an inner statement rendered as a bare `"rel:10"` pointer -- so
 * the one surface a model ever reads got the parts and lost the whole.
 *
 * `subject: rel:N` is NOT nesting. That is a meta ABOUT a statement (provenance,
 * modality); it is already carried by the sources/modal machinery, and expanding
 * it would inline the target statement beneath every one of the 1167 source
 * metas in the trial store. `derived_from` and `supersedes` are versioning
 * bookkeeping, surfaced through history and superseded_by. Everything else that
 * points at a statement is content. */
static int frame_slot_nests(const Attr *at) {
  return at->value.tag == TERM_REL && at->name != WK_SUBJECT &&
         at->name != WK_DERIVED_FROM && at->name != WK_SUPERSEDES;
}

/* Nesting depth a frame will expand. 1 covers a directive; 2 covers "A asked B
 * to ask C". Past the budget the pointer stands, so no chain can balloon a
 * frame -- orientation-packet size is already the live constraint (#91). */
enum { FRAME_NEST_MAX = 2 };

static void frame_put_rel_attrs_n(const Hypergraph *g, u32 rid, u32 budget) {
  const Relation *r = &g->relations[rid];
  u32 a;
  fputc('{', stdout);
  for (a = 0; a < r->attr_count; a++) {
    if (a)
      fputc(',', stdout);
    fputc('"', stdout);
    json_escape_fputs(elem_name(g, r->attrs[a].name), stdout);
    fputs("\":", stdout);
    if (r->attrs[a].value.tag == TERM_ELEM) {
      fputc('"', stdout);
      json_escape_fputs(elem_name(g, r->attrs[a].value.id), stdout);
      fputc('"', stdout);
    } else if (budget && frame_slot_nests(&r->attrs[a])) {
      u32 in = r->attrs[a].value.id;
      printf("{\"ref\":\"rel:%u\",\"attrs\":", in);
      frame_put_rel_attrs_n(g, in, budget - 1);
      /* The inner statement's modality is the whole difference between "asked
       * for" and "is true": a `non_actual` inner claim rendered bare reads as
       * an assertion, the same inversion frame_put_rel_entry guards against.
       * Status likewise -- a retracted inner statement must not read live.
       * Both emitted only when they say something, to keep nesting cheap. */
      if (rel_modal_count(g, in)) {
        fputs(",\"modal\":[", stdout);
        frame_put_modal_values(g, in);
        fputc(']', stdout);
      }
      if (g->relations[in].status != ST_ASSERTED)
        printf(",\"status\":\"%s\"", ST_NAMES[g->relations[in].status]);
      fputc('}', stdout);
    } else {
      printf("\"rel:%u\"", r->attrs[a].value.id);
    }
  }
  fputc('}', stdout);
}

static void frame_put_rel_attrs(const Hypergraph *g, u32 rid) {
  frame_put_rel_attrs_n(g, rid, FRAME_NEST_MAX);
}

/* The uniform relation-object shape (pin §3.22). */
/* A modal is one of the five §16.3 modality markers: intervened (do()), plus
 * the extended negated/uncertain/non_actual/general. */
static int is_modal_name(const Hypergraph *g, u32 id) {
  return id == WK_INTERVENED || id == g->wk_ext[EXT_NEGATED] ||
         id == g->wk_ext[EXT_UNCERTAIN] || id == g->wk_ext[EXT_NON_ACTUAL] ||
         id == g->wk_ext[EXT_GENERAL];
}

/* If meta-relation m_rid is a live modal self-marker on target_rid -- its
 * subject slot IS target_rid and it carries a modal attr -- return that modal
 * element id, else NONE_U32. This is how a fact's `modal` array is reified
 * (§7.2): each modal becomes a `[subject:<fact-rel>, <modal>:<modal>]`. */
static u32 meta_modal_of(const Hypergraph *g, u32 m_rid, u32 target_rid) {
  const Relation *m = &g->relations[m_rid];
  u32 a, ma = NONE_U32;
  int subj_is_target = 0;
  if (m->status >= ST_SUPERSEDED)
    return NONE_U32;
  for (a = 0; a < m->attr_count; a++) {
    if (m->attrs[a].name == WK_SUBJECT && m->attrs[a].value.tag == TERM_REL &&
        m->attrs[a].value.id == target_rid)
      subj_is_target = 1;
    else if (is_modal_name(g, m->attrs[a].name))
      ma = m->attrs[a].name;
  }
  return subj_is_target ? ma : NONE_U32;
}

/* How many live modal markers rid carries. */
static u32 rel_modal_count(const Hypergraph *g, u32 rid) {
  u32 link, n = 0;
  for (link = g->meta_by_target[rid]; link != NONE_U32;
       link = g->rel_links[link].next)
    if (meta_modal_of(g, g->rel_links[link].rel, rid) != NONE_U32)
      n++;
  return n;
}

/* Emit rid's modal names as comma-separated JSON strings (no brackets). */
static void frame_put_modal_values(const Hypergraph *g, u32 rid) {
  u32 link, put = 0;
  for (link = g->meta_by_target[rid]; link != NONE_U32;
       link = g->rel_links[link].next) {
    u32 ma = meta_modal_of(g, g->rel_links[link].rel, rid);
    if (ma == NONE_U32)
      continue;
    if (put++)
      fputc(',', stdout);
    fputc('"', stdout);
    json_escape_fputs(elem_name(g, ma), stdout);
    fputc('"', stdout);
  }
}

static void frame_put_rel_entry(const Hypergraph *g, u32 rid, int first) {
  const Relation *r = &g->relations[rid];
  char conf[8];
  fmt_unit_float(r->stats.confidence, conf);
  if (!first)
    fputc(',', stdout);
  printf("{\"ref\":\"rel:%u\",\"attrs\":", rid);
  frame_put_rel_attrs(g, rid);
  printf(
      ",\"status\":\"%s\",\"confidence\":%s,\"support_count\":%u,\"date\":\"",
      ST_NAMES[r->status], conf, r->stats.support_count);
  frame_put_day(g, r->created_at);
  fputc('"', stdout);
  /* Modality is a property of the CLAIM, so a negated/uncertain/non_actual fact
   * must show its modal here too -- not only in the causal section -- or a
   * `modal:["negated"]` fact reads as a plain asserted one (an inverted claim).
   * Emitted only when present: most facts carry none, and the token cost of an
   * empty "modal":[] on every entry is not worth paying. */
  if (rel_modal_count(g, rid)) {
    fputs(",\"modal\":[", stdout);
    frame_put_modal_values(g, rid);
    fputc(']', stdout);
  }
  fputc('}', stdout);
}

/* ---- full-frame section assembly ---- */

/* The builtin top-level frame keys print_frame emits. A template or instance
 * kind whose normalized name equals one of these would emit a duplicate JSON
 * key, so it is rejected at plan time (spec §5). Kept beside the emitter so the
 * list cannot drift from what the frame actually writes. */
static const char *const FRAME_RESERVED_KEYS[] = {
    "tick",         "at",        "store",          "resolution",    "writes",
    "near_matches", "conflicts", "template_drift", "focus",         "overview",
    "state",        "decisions", "constraints",    "open",          "recent",
    "history",      "related",   "pointers",       "sources",       "causal",
    "omitted",      "foreign_build"};

/* The build stamp of another process that wrote this store while we held it
 * warm. A re-pin swaps the binary on disk but leaves running servers on their
 * old image (docs/alchamancer-trial.md), so an ended session's server can keep
 * serving saves for days: one stale image wrote 60 journal lines over 2.5 days
 * of the last round, interleaved with the new build 17 times. Empty when every
 * writer has matched us. */
static char g_foreign_build[24];

/* Entries per typed section (decisions, constraints, open, causal, and each
 * custom-kind section). These were emitted UNCAPPED — `limit` budgets only
 * recent/related — on the reasoning that the typed sections are the ones worth
 * never truncating. That holds at 10 decisions and fails at 103: the live
 * trial packet reached 51KB, of which decisions alone were 18KB and
 * constraints 12KB, while the SessionStart hook feeds the model the first 4000
 * bytes. The sections meant to be protected were the ones being dropped.
 *
 * A cap is a surface bound, never a store one: nothing is pruned, the entries
 * are newest-first, and whatever is held back is counted in `omitted` so the
 * model can see there is more and recall for it. */
static u32 g_frame_section_cap = 10;

static int is_reserved_frame_key(const char *norm, u32 nlen) {
  u32 i;
  char kbuf[32];
  for (i = 0; i < sizeof FRAME_RESERVED_KEYS / sizeof FRAME_RESERVED_KEYS[0];
       i++) {
    u32 klen = normalize_name(FRAME_RESERVED_KEYS[i],
                              (u32)strlen(FRAME_RESERVED_KEYS[i]), kbuf);
    if (klen == nlen && memcmp(kbuf, norm, nlen) == 0)
      return 1;
  }
  return 0;
}

/* meta relations and instance_of typing never surface as section entries
 * (spec §7: they surface through sources/pointers/history and kinds) */
static int rel_is_meta(const Hypergraph *g, u32 rid) {
  const Relation *r = &g->relations[rid];
  u32 a;
  for (a = 0; a < r->attr_count; a++) {
    if (r->attrs[a].name == WK_SUBJECT && r->attrs[a].value.tag == TERM_REL)
      return 1;
    if (r->attrs[a].name == WK_INSTANCE_OF)
      return 1;
  }
  return 0;
}

static int name_is_current(const Hypergraph *g, u32 name_elem) {
  const char *nb = elem_name(g, name_elem);
  return elem_name_l(g, name_elem) > 8 && memcmp(nb, "current_", 8) == 0;
}

/* A current_* cache: element-valued subject plus a current_-named slot;
 * returns the subject element or NONE_U32. */
static u32 rel_cache_subject(const Hypergraph *g, u32 rid) {
  const Relation *r = &g->relations[rid];
  u32 a, subj = NONE_U32;
  int cur = 0;
  for (a = 0; a < r->attr_count; a++) {
    if (r->attrs[a].name == WK_SUBJECT && r->attrs[a].value.tag == TERM_ELEM)
      subj = r->attrs[a].value.id;
    else if (name_is_current(g, r->attrs[a].name))
      cur = 1;
  }
  return cur ? subj : NONE_U32;
}

/* Section ordering (pin §3.18): date desc, then id desc. */
static const Hypergraph *g_sort_g;
static int rel_recency_cmp(const void *pa, const void *pb) {
  u32 a = *(const u32 *)pa, b = *(const u32 *)pb;
  i64 da = frame_tick_day(g_sort_g, g_sort_g->relations[a].created_at);
  i64 db = frame_tick_day(g_sort_g, g_sort_g->relations[b].created_at);
  if (da != db)
    return da > db ? -1 : 1;
  return a > b ? -1 : a < b ? 1 : 0;
}
static int elem_recency_cmp(const void *pa, const void *pb) {
  u32 a = *(const u32 *)pa, b = *(const u32 *)pb;
  i64 da = frame_tick_day(g_sort_g, g_sort_g->elements[a].created_at);
  i64 db = frame_tick_day(g_sort_g, g_sort_g->elements[b].created_at);
  if (da != db)
    return da > db ? -1 : 1;
  return a > b ? -1 : a < b ? 1 : 0;
}

/* Orientation `active` ranking (spec §6): activation x recency — the live
 * dynamics deciding what "matters now". recency = 1 / (1 + ticks since last
 * touch), so a burst of old activity cannot outrank what this week actually
 * touched; ties last_seen desc, then id desc. */
static int elem_active_cmp(const void *pa, const void *pb) {
  u32 a = *(const u32 *)pa, b = *(const u32 *)pb;
  const Stats *sa = &g_sort_g->elements[a].stats,
              *sb = &g_sort_g->elements[b].stats;
  double xa = sa->activation / (double)(1 + g_sort_g->clock - sa->last_seen);
  double xb = sb->activation / (double)(1 + g_sort_g->clock - sb->last_seen);
  if (xa != xb)
    return xa > xb ? -1 : 1;
  if (sa->last_seen != sb->last_seen)
    return sa->last_seen > sb->last_seen ? -1 : 1;
  return a > b ? -1 : a < b ? 1 : 0;
}

/* Live plain attrs {subject: e, <attr>: v} of an instance plus its live
 * current_* caches, rel ids ascending; excludes typing and pointer links
 * (they have their own sections). Caches join so the instance shape can
 * denormalize a changed property back to its attribute name ("standing"
 * from current_standing). */
static void collect_instance_attrs(const Hypergraph *g, u32 e, U32Vec *out) {
  u32 link = g->rels_by_elem[e];
  out->count = 0;
  while (link != NONE_U32) {
    u32 rid = g->rel_links[link].rel;
    link = g->rel_links[link].next;
    {
      const Relation *r = &g->relations[rid];
      u32 a, name = NONE_U32;
      int subj_is_e = 0;
      if (r->status >= ST_SUPERSEDED || r->attr_count != 2)
        continue;
      for (a = 0; a < 2; a++) {
        if (r->attrs[a].name == WK_SUBJECT &&
            r->attrs[a].value.tag == TERM_ELEM && r->attrs[a].value.id == e)
          subj_is_e = 1;
        else if (r->attrs[a].name != WK_SUBJECT)
          name = r->attrs[a].name;
      }
      if (!subj_is_e || name == NONE_U32)
        continue;
      if (name == WK_INSTANCE_OF || name == WK_SRC || name == WK_EXPECTS)
        continue;
      u32vec_push_unique(out, rid);
    }
  }
  u32vec_sort(out, u32_cmp);
}

/* The instance-shape display name of an attr-name element: current_* caches
 * denormalize back to the bare property name. */
static const char *attr_display_name(const Hypergraph *g, u32 name_elem) {
  const char *nb = elem_name(g, name_elem);
  return name_is_current(g, name_elem) ? nb + 8 : nb;
}

static u32 rel_pair_name(const Hypergraph *g, u32 rid) {
  const Relation *r = &g->relations[rid];
  return r->attrs[0].name == WK_SUBJECT ? r->attrs[1].name : r->attrs[0].name;
}

/* Render a base fact as "subject predicate object" so a query can be
 * cosine-matched against it (F1 relevance ranking). */
static void frame_rel_text(const Hypergraph *g, u32 rid, char *buf, size_t cap) {
  const Relation *r = &g->relations[rid];
  const char *subj = "", *pred = "", *obj = "";
  if (r->attr_count == 2) {
    u32 sa = r->attrs[0].name == WK_SUBJECT ? 0 : 1, pa = sa ^ 1;
    const Term *ov = &r->attrs[pa].value;
    if (r->attrs[sa].value.tag == TERM_ELEM)
      subj = elem_name(g, r->attrs[sa].value.id);
    pred = attr_display_name(g, r->attrs[pa].name);
    if (ov->tag == TERM_ELEM)
      obj = elem_name(g, ov->id);
  } else if (r->attr_count > 0) {
    pred = elem_name(g, r->attrs[0].name);
  }
  snprintf(buf, cap, "%s %s %s", subj, pred, obj);
}

/* Instance shape: {ref, name, kind?, <attr>: value | [values], date} —
 * multi-value attrs render as arrays, relation id ascending (shape rule §5). */
static void frame_put_instance(const Hypergraph *g, u32 e, int with_kind,
                               int first) {
  static U32Vec attrs;
  u32 i, j;
  if (!first)
    fputc(',', stdout);
  printf("{\"ref\":\"#%u\",\"name\":\"", e);
  json_escape_fputs(elem_name(g, e), stdout);
  fputc('"', stdout);
  if (with_kind && g->elem_kind[e] != NONE_U32) {
    fputs(",\"kind\":\"", stdout);
    json_escape_fputs(elem_name(g, g->elem_kind[e]), stdout);
    fputc('"', stdout);
  }
  collect_instance_attrs(g, e, &attrs);
  for (i = 0; i < attrs.count; i++) {
    const char *disp = attr_display_name(g, rel_pair_name(g, attrs.v[i]));
    u32 nvals = 0;
    int emitted_before = 0;
    for (j = 0; j < i; j++)
      if (strcmp(attr_display_name(g, rel_pair_name(g, attrs.v[j])), disp) == 0)
        emitted_before = 1;
    if (emitted_before)
      continue;
    for (j = i; j < attrs.count; j++)
      if (strcmp(attr_display_name(g, rel_pair_name(g, attrs.v[j])), disp) == 0)
        nvals++;
    fputs(",\"", stdout);
    json_escape_fputs(disp, stdout);
    fputs("\":", stdout);
    if (nvals > 1)
      fputc('[', stdout);
    {
      u32 put = 0;
      for (j = i; j < attrs.count; j++) {
        const Relation *p = &g->relations[attrs.v[j]];
        const Term *v;
        if (strcmp(attr_display_name(g, rel_pair_name(g, attrs.v[j])), disp) !=
            0)
          continue;
        v = p->attrs[0].name == WK_SUBJECT ? &p->attrs[1].value
                                           : &p->attrs[0].value;
        if (put++)
          fputc(',', stdout);
        if (v->tag == TERM_ELEM) {
          fputc('"', stdout);
          json_escape_fputs(elem_name(g, v->id), stdout);
          fputc('"', stdout);
        } else {
          printf("\"rel:%u\"", v->id);
        }
      }
    }
    if (nvals > 1)
      fputc(']', stdout);
  }
  fputs(",\"date\":\"", stdout);
  frame_put_day(g, g->elements[e].created_at);
  fputs("\"}", stdout);
}

/* An element carrying a live current_standing cache whose value is not
 * "active" leaves the constraints section; no cache means nothing says the
 * constraint was lifted. */
static int constraint_is_active(const Hypergraph *g, u32 e) {
  u32 link = g->rels_by_elem[e];
  while (link != NONE_U32) {
    u32 rid = g->rel_links[link].rel;
    link = g->rel_links[link].next;
    {
      const Relation *r = &g->relations[rid];
      u32 a, val = NONE_U32;
      int subj_is_e = 0, is_standing = 0;
      if (r->status >= ST_SUPERSEDED || r->attr_count != 2)
        continue;
      for (a = 0; a < 2; a++) {
        if (r->attrs[a].name == WK_SUBJECT &&
            r->attrs[a].value.tag == TERM_ELEM && r->attrs[a].value.id == e)
          subj_is_e = 1;
        else if (elem_name_l(g, r->attrs[a].name) == 16 &&
                 memcmp(elem_name(g, r->attrs[a].name), "current_standing",
                        16) == 0) {
          is_standing = 1;
          if (r->attrs[a].value.tag == TERM_ELEM)
            val = r->attrs[a].value.id;
        }
      }
      if (!subj_is_e || !is_standing)
        continue;
      if (val == NONE_U32)
        return 0;
      return elem_name_l(g, val) == 6 &&
             memcmp(elem_name(g, val), "active", 6) == 0;
    }
  }
  return 1;
}

static int elem_is_open(const Hypergraph *g, u32 e) {
  u32 link = g->rels_by_elem[e];
  while (link != NONE_U32) {
    u32 rid = g->rel_links[link].rel;
    link = g->rel_links[link].next;
    {
      const Relation *r = &g->relations[rid];
      u32 a;
      if (r->status >= ST_SUPERSEDED)
        continue;
      for (a = 0; a < r->attr_count; a++)
        if (r->attrs[a].name == WK_RESOLVES &&
            r->attrs[a].value.tag == TERM_ELEM && r->attrs[a].value.id == e)
          return 0;
    }
  }
  return 1;
}

/* A template kind carries at least one live expects relation. */
static int kind_has_template(const Hypergraph *g, u32 kind) {
  u32 link = g->rels_by_elem[kind];
  while (link != NONE_U32) {
    u32 rid = g->rel_links[link].rel;
    link = g->rel_links[link].next;
    {
      const Relation *r = &g->relations[rid];
      u32 a;
      int subj_is_kind = 0, has_expects = 0;
      if (r->status >= ST_SUPERSEDED || r->attr_count != 2)
        continue;
      for (a = 0; a < 2; a++) {
        if (r->attrs[a].name == WK_SUBJECT &&
            r->attrs[a].value.tag == TERM_ELEM && r->attrs[a].value.id == kind)
          subj_is_kind = 1;
        if (r->attrs[a].name == WK_EXPECTS)
          has_expects = 1;
      }
      if (subj_is_kind && has_expects)
        return 1;
    }
  }
  return 0;
}

/* Focus-shaped entry {ref, name, kind?, summary?} — kind/summary omitted
 * when absent (the pin §3.15 exception to the no-omission rule). */
static void frame_put_focus_entry(const Hypergraph *g, u32 e) {
  printf("{\"ref\":\"#%u\",\"name\":\"", e);
  json_escape_fputs(elem_name(g, e), stdout);
  fputc('"', stdout);
  if (g->elem_kind[e] != NONE_U32) {
    fputs(",\"kind\":\"", stdout);
    json_escape_fputs(elem_name(g, g->elem_kind[e]), stdout);
    fputc('"', stdout);
  }
  if (g->elements[e].summary != NONE_U32) {
    fputs(",\"summary\":\"", stdout);
    json_escape_fputs(str_ptr(&g->strs, g->elements[e].summary), stdout);
    fputc('"', stdout);
  }
  fputc('}', stdout);
}

/* Candidate object {ref, name, kind?, score} (pin §3.15). */
static void frame_put_candidate(const Hypergraph *g, u32 e, double score,
                                int first) {
  char sc[8];
  fmt_unit_float(score, sc);
  if (!first)
    fputc(',', stdout);
  printf("{\"ref\":\"#%u\",\"name\":\"", e);
  json_escape_fputs(elem_name(g, e), stdout);
  fputc('"', stdout);
  if (g->elem_kind[e] != NONE_U32) {
    fputs(",\"kind\":\"", stdout);
    json_escape_fputs(elem_name(g, g->elem_kind[e]), stdout);
    fputc('"', stdout);
  }
  printf(",\"score\":%s}", sc);
}

/* Reciprocal-rank fusion, shared by rank_related and rank_recent_novelty:
 * each contributing list adds rrf_term(rank), so being #1 in a list is worth
 * much more than #2, and lists simply add. k=60 is the standard RRF damping
 * constant; a large k keeps any single list from dominating. No score
 * magnitudes, only ordinal ranks, so incomparable stat scales fuse cleanly.
 * Ranks are competition ranks (1 + count strictly better), so all-equal
 * stats tie instead of leaking id order through ordinal ranks. */
typedef struct {
  u32 rid;
  double rrf;
  i64 tie; /* recency tie-break, desc: last_seen (related) or day (recent) */
} RRFEntry;

static double rrf_term(u32 rank) { return 1.0 / (60.0 + (double)rank); }

/* Final id tie-break direction: related follows candidate order (plan §5,
 * id asc); recent follows section recency order (pin §3.18, id desc). */
static int g_rrf_id_desc;
static int rrf_cmp(const void *pa, const void *pb) {
  const RRFEntry *a = (const RRFEntry *)pa, *b = (const RRFEntry *)pb;
  if (a->rrf != b->rrf)
    return a->rrf > b->rrf ? -1 : 1;
  if (a->tie != b->tie)
    return a->tie > b->tie ? -1 : 1;
  if (a->rid == b->rid)
    return 0;
  if (g_rrf_id_desc)
    return a->rid > b->rid ? -1 : 1;
  return a->rid < b->rid ? -1 : 1;
}

/* Sort rr by (rrf desc, tie desc, id) and write the order back into rels. */
static void rrf_apply(U32Vec *rels, RRFEntry *rr, int id_desc) {
  u32 i;
  g_rrf_id_desc = id_desc;
  qsort(rr, rels->count, sizeof *rr, rrf_cmp);
  for (i = 0; i < rels->count; i++)
    rels->v[i] = rr[i].rid;
}

/* related ranking (pin §3.4): fuse rank-by-activation and rank-by-
 * focus_success_count; ties last_seen desc, then id asc. */
static void rank_related(const Hypergraph *g, U32Vec *rels) {
  static RRFEntry *rr;
  static u32 rr_cap;
  u32 i, j, n = rels->count;
  if (n < 2)
    return;
  rr = (RRFEntry *)xgrow(rr, n, &rr_cap, sizeof *rr);
  for (i = 0; i < n; i++) {
    const Stats *si = &g->relations[rels->v[i]].stats;
    u32 rank_act = 1, rank_fsc = 1;
    for (j = 0; j < n; j++) {
      const Stats *sj = &g->relations[rels->v[j]].stats;
      if (sj->activation > si->activation)
        rank_act++;
      if (sj->focus_success_count > si->focus_success_count)
        rank_fsc++;
    }
    rr[i].rid = rels->v[i];
    rr[i].rrf = rrf_term(rank_act) + rrf_term(rank_fsc);
    rr[i].tie = (i64)si->last_seen;
  }
  rrf_apply(rels, rr, 0);
}

/* F1 embeds every candidate fact at query time (embed_rank_texts is uncached, by
 * design so recall stays read-only). Each BGE forward pass is ~20ms, so a large
 * neighborhood would blow past a second. To stay sub-second AND keep quality, a
 * cheap lexical pre-filter picks the F1_RERANK_CAP facts with the most query-term
 * overlap (microseconds), and only those are embedded and cosine-ranked. Lexical
 * overlap — not recency — is the selector precisely because F1 exists to surface
 * a LOW-recency fact; a recency cap would re-exclude the answer. The un-embedded
 * facts keep their incoming order after the ranked head (never dropped). */
static const u32 F1_RERANK_CAP = 32;

static char f1_lower(char c) { return (c >= 'A' && c <= 'Z') ? (char)(c + 32) : c; }
static int f1_alnum(char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
}

/* F1: reorder a relation band by cosine relevance of each fact to the focus
 * query. No-op unless the embedder is available, so the LEGEND_EMBED=0
 * determinism gate keeps its recency ordering. */
static void rerank_relevance(const Hypergraph *g, U32Vec *rels,
                             const char *query) {
  static char (*txt)[256], (*low)[256];
  static u32 txt_cap, low_cap;
  static const char **texts;
  static u32 texts_cap;
  static u32 *sel, *sc;
  static u32 sel_cap, sc_cap;
  static u32 *ids, *out_ids;
  static u32 ids_cap, out_ids_cap;
  static float *out_sc;
  static u32 out_sc_cap;
  static U32Vec ranked;
  char qtok[24][32];
  u32 n = rels->count, m, i, j, nq = 0;
  int r;
  if (!query || !*query || n < 2 || !embed_available())
    return;

  /* render + lowercase every candidate fact (cheap; no embedding yet) */
  txt = (char(*)[256])xgrow(txt, n, &txt_cap, sizeof *txt);
  low = (char(*)[256])xgrow(low, n, &low_cap, sizeof *low);
  for (i = 0; i < n; i++) {
    frame_rel_text(g, rels->v[i], txt[i], sizeof txt[i]);
    for (j = 0; txt[i][j] && j < 255; j++)
      low[i][j] = f1_lower(txt[i][j]);
    low[i][j] = 0;
  }
  /* tokenize the query (lowercased, len>=3) */
  for (i = 0; query[i] && nq < 24;) {
    while (query[i] && !f1_alnum(query[i]))
      i++;
    j = 0;
    while (query[i] && f1_alnum(query[i])) {
      if (j < 31)
        qtok[nq][j++] = f1_lower(query[i]);
      i++;
    }
    if (j >= 3) {
      qtok[nq][j] = 0;
      nq++;
    }
  }

  /* pick the facts to embed: all of them, or the top-CAP by query-term overlap */
  sel = (u32 *)xgrow(sel, n, &sel_cap, sizeof *sel);
  for (i = 0; i < n; i++)
    sel[i] = i;
  if (n > F1_RERANK_CAP && nq > 0) {
    sc = (u32 *)xgrow(sc, n, &sc_cap, sizeof *sc);
    for (i = 0; i < n; i++) {
      u32 s = 0;
      for (j = 0; j < nq; j++)
        if (strstr(low[i], qtok[j]))
          s++;
      sc[i] = s;
    }
    for (i = 0; i < F1_RERANK_CAP; i++) { /* partial selection of the top CAP */
      u32 best = i, t;
      for (j = i + 1; j < n; j++)
        if (sc[sel[j]] > sc[sel[best]])
          best = j;
      t = sel[i];
      sel[i] = sel[best];
      sel[best] = t;
    }
    m = F1_RERANK_CAP;
  } else {
    m = n;
  }

  /* Rank via embed_rank_elements (keyed by rel id + text hash) so each fact's
   * query-independent vector is embedded once and cached in vectors.bin; warm
   * recalls are cosine dot-products, not forward passes. */
  texts = (const char **)xgrow(texts, m, &texts_cap, sizeof *texts);
  ids = (u32 *)xgrow(ids, m, &ids_cap, sizeof *ids);
  out_ids = (u32 *)xgrow(out_ids, m, &out_ids_cap, sizeof *out_ids);
  out_sc = (float *)xgrow(out_sc, m, &out_sc_cap, sizeof *out_sc);
  for (i = 0; i < m; i++) {
    texts[i] = txt[sel[i]];
    ids[i] = rels->v[sel[i]];
  }
  r = embed_rank_elements(ids, texts, (int)m, query, (int)strlen(query), out_ids,
                          out_sc, (int)m);
  if (r <= 0)
    return; /* embedder failed -> keep recency order */

  ranked.count = 0;
  for (i = 0; i < (u32)r; i++) /* relevance-ranked head (rel ids) */
    u32vec_push(&ranked, out_ids[i]);
  for (i = 0; i < n; i++) { /* everything else, incoming order */
    u32 rid = rels->v[i], k;
    int found = 0;
    for (k = 0; k < (u32)r; k++)
      if (out_ids[k] == rid) {
        found = 1;
        break;
      }
    if (!found)
      u32vec_push(&ranked, rid);
  }
  rels->count = 0;
  for (i = 0; i < ranked.count; i++)
    u32vec_push(rels, ranked.v[i]);
}

/* recent-band novelty bias (intent curiosity): the recent band is recency by
 * default, but a submitted curiosity > 0 fuses in two novelty rankings —
 * coldness (low activation) and under-exploration (low focus_success_count) —
 * so unfamiliar neighbors climb into the capped band. curiosity weights the
 * novelty lists, so it dials continuously from pure recency (0) toward the
 * unfamiliar; ties day desc, then id desc. */
static void rank_recent_novelty(const Hypergraph *g, U32Vec *rels,
                                double curiosity) {
  static RRFEntry *rr;
  static u32 rr_cap;
  u32 i, j, n = rels->count;
  if (n < 2)
    return;
  rr = (RRFEntry *)xgrow(rr, n, &rr_cap, sizeof *rr);
  for (i = 0; i < n; i++) {
    const Stats *si = &g->relations[rels->v[i]].stats;
    i64 day_i = frame_tick_day(g, g->relations[rels->v[i]].created_at);
    u32 rank_rec = 1, rank_cold = 1, rank_under = 1;
    for (j = 0; j < n; j++) {
      const Stats *sj = &g->relations[rels->v[j]].stats;
      i64 day_j = frame_tick_day(g, g->relations[rels->v[j]].created_at);
      if (day_j > day_i)
        rank_rec++;
      if (sj->activation < si->activation)
        rank_cold++;
      if (sj->focus_success_count < si->focus_success_count)
        rank_under++;
    }
    rr[i].rid = rels->v[i];
    rr[i].rrf = rrf_term(rank_rec) +
                curiosity * (rrf_term(rank_cold) + rrf_term(rank_under));
    rr[i].tie = day_i;
  }
  rrf_apply(rels, rr, 1);
}

typedef struct {
  u32 rel, by, at_tick;
} HistEntry;

/* Is `id` a behavioral-modal attribute-name element (§16.3)? */
/* If `rid` is a causal edge -- carries an attr named caused/enables/prevents/
 * correlated_with (§16.3) -- return that predicate's element id, else NONE_U32. */
static u32 rel_causal_pred(const Hypergraph *g, u32 rid) {
  const Relation *r = &g->relations[rid];
  u32 a;
  for (a = 0; a < r->attr_count; a++) {
    u32 nm = r->attrs[a].name;
    if (nm == g->wk_ext[EXT_CAUSED] || nm == g->wk_ext[EXT_ENABLES] ||
        nm == g->wk_ext[EXT_PREVENTS] || nm == g->wk_ext[EXT_CORRELATED_WITH])
      return nm;
  }
  return NONE_U32;
}

/* One causal-section entry: the edge's attrs, its Pearl rung (correlated_with is
 * rung-1 correlational; caused/enables/prevents are rung-2 causal), and the
 * modality carried by its meta-relations (§16.3) so a counterfactual, negated,
 * or intervened causal claim is not read as a plain actual one. */
static void frame_put_causal_entry(const Hypergraph *g, u32 rid, int first) {
  u32 pred = rel_causal_pred(g, rid);
  const char *rung =
      pred == g->wk_ext[EXT_CORRELATED_WITH] ? "correlational" : "causal";
  if (!first)
    fputc(',', stdout);
  printf("{\"ref\":\"rel:%u\",\"attrs\":", rid);
  frame_put_rel_attrs(g, rid);
  fputs(",\"rung\":\"", stdout);
  fputs(rung, stdout);
  fputs("\",\"modal\":[", stdout);
  frame_put_modal_values(g, rid);
  fputs("]}", stdout);
}

/* The frame (spec §7). A compact frame (no focus) stops after template_drift;
 * a full frame appends the packet sections. Empty fields emit as [] per plan
 * §3.14; key order is fixed (plan §5). */
/* The orientation overview carries a store-health tally; the checks it counts
 * live with the rest of the audit in S11.5, below the frame code. */
static void frame_put_audit_tally(const Hypergraph *g);

static void print_frame(const Hypergraph *g, const WriteReport *w,
                        const char *store, i64 limit, i32 history_depth,
                        i64 since) {
  char at[40];
  u32 i, j;
  format_iso_utc(w->at_secs, at);
  printf("{\"tick\":%u,\"at\":\"%s\",\"store\":\"", w->tick, at);
  json_escape_fputs(store, stdout);
  fputc('"', stdout);
  if (g_foreign_build[0]) {
    fputs(",\"foreign_build\":\"", stdout);
    json_escape_fputs(g_foreign_build, stdout);
    fputc('"', stdout);
  }
  fputs(",\"resolution\":[", stdout);
  for (i = 0; i < w->res_count; i++) {
    const ResEntry *e = &w->res[i];
    if (i)
      fputc(',', stdout);
    printf("{\"at\":\"%s\",\"submitted\":\"", e->at);
    json_escape_fwrite(w->payload + e->submitted.off, e->submitted.len, stdout);
    fputc('"', stdout);
    if (e->resolved) {
      char sc[8];
      fmt_unit_float(e->score, sc);
      printf(",\"ref\":\"#%u\",\"name\":\"", e->elem);
      json_escape_fputs(elem_name(g, e->elem), stdout);
      printf("\",\"via\":\"%s\",\"score\":%s}",
             e->via == VIA_HOMONYM ? "homonym" : "lexical", sc);
    } else {
      fputs(",\"resolved\":false,\"candidates\":[", stdout);
      for (j = 0; j < e->cand_count; j++)
        frame_put_candidate(g, w->cands.v[e->cand_start + j].elem,
                            w->cands.v[e->cand_start + j].score, j == 0);
      fputs("]}", stdout);
    }
  }
  fputs("],\"writes\":{\"minted_elements\":[", stdout);
  for (i = 0; i < w->minted_elems.count; i++)
    frame_put_elem_entry(g, w->minted_elems.v[i], i == 0);
  fputs("],\"reused_elements\":[", stdout);
  for (i = 0; i < w->reused_elems.count; i++)
    frame_put_elem_entry(g, w->reused_elems.v[i], i == 0);
  fputs("],\"minted_relations\":[", stdout);
  for (i = 0; i < w->minted_rels.count; i++)
    printf("%s\"rel:%u\"", i ? "," : "", w->minted_rels.v[i]);
  fputs("],\"reused_relations\":[", stdout);
  for (i = 0; i < w->reused_rel_count; i++)
    printf("%s{\"ref\":\"rel:%u\",\"support_count\":%u,\"promoted\":%s}",
           i ? "," : "", w->reused_rels[i].rel, w->reused_rels[i].support_count,
           w->reused_rels[i].promoted ? "true" : "false");
  fputs("],\"retracted\":[", stdout);
  for (i = 0; i < w->retracted.count; i++)
    printf("%s\"rel:%u\"", i ? "," : "", w->retracted.v[i]);
  fputs("],\"merged\":[", stdout);
  for (i = 0; i < w->merge_count; i++) {
    if (i)
      fputc(',', stdout);
    fputs("{\"from\":\"", stdout);
    json_escape_fwrite(w->payload + w->merges[i].from.off,
                       w->merges[i].from.len, stdout);
    fputs("\",\"into\":\"", stdout);
    json_escape_fwrite(w->payload + w->merges[i].into.off,
                       w->merges[i].into.len, stdout);
    fputs("\"}", stdout);
  }
  fputs("]},\"near_matches\":[", stdout);
  for (i = 0; i < w->near_count; i++) {
    const NearEntry *e = &w->nears[i];
    char sc[8];
    fmt_unit_float(e->score, sc);
    if (i)
      fputc(',', stdout);
    printf("{\"at\":\"%s\",\"minted\":{\"ref\":\"#%u\",\"name\":\"", e->at,
           e->minted);
    json_escape_fputs(elem_name(g, e->minted), stdout);
    fputc('"', stdout);
    if (g->elem_kind[e->minted] != NONE_U32) {
      fputs(",\"kind\":\"", stdout);
      json_escape_fputs(elem_name(g, g->elem_kind[e->minted]), stdout);
      fputc('"', stdout);
    }
    printf("},\"existing\":{\"ref\":\"#%u\",\"name\":\"", e->existing);
    json_escape_fputs(elem_name(g, e->existing), stdout);
    fputc('"', stdout);
    if (g->elem_kind[e->existing] != NONE_U32) {
      fputs(",\"kind\":\"", stdout);
      json_escape_fputs(elem_name(g, g->elem_kind[e->existing]), stdout);
      fputc('"', stdout);
    }
    printf("},\"score\":%s}", sc);
  }
  fputs("],\"conflicts\":[", stdout);
  for (i = 0; i < w->conflict_count; i++) {
    const ConflictEntry *e = &w->conflicts[i];
    if (i)
      fputc(',', stdout);
    printf("{\"change\":\"rel:%u\",\"prior\":\"rel:%u\",\"property\":\"",
           e->event_rel, e->prior_rel);
    json_escape_fputs(elem_name(g, e->property_elem), stdout);
    fputs("\",\"values\":[", stdout);
    if (e->pval_tag == TERM_ELEM) {
      fputc('"', stdout);
      json_escape_fputs(elem_name(g, e->pval_id), stdout);
      fputc('"', stdout);
    } else {
      printf("\"rel:%u\"", e->pval_id);
    }
    fputs(",\"", stdout);
    json_escape_fputs(elem_name(g, e->to_val), stdout);
    fputs("\"]}", stdout);
  }
  fputs("],\"template_drift\":[", stdout);
  for (i = 0; i < w->drift_count; i++) {
    const DriftEntry *e = &w->drifts[i];
    if (i)
      fputc(',', stdout);
    printf("{\"instance\":\"#%u\",\"kind\":\"", e->elem);
    json_escape_fputs(elem_name(g, e->kind_elem), stdout);
    fputs("\",\"unexpected\":[", stdout);
    for (j = 0; j < e->count; j++) {
      if (j)
        fputc(',', stdout);
      fputc('"', stdout);
      json_escape_fputs(elem_name(g, e->keys[j]), stdout);
      fputc('"', stdout);
    }
    fputs("]}", stdout);
  }
  fputc(']', stdout);
  if (!w->has_focus) {
    fputs("}\n", stdout);
    return;
  }

  /* ---- the packet sections ---- */
  {
    static U32Vec hood, pool, state, recent, related, nbr, sect, consumed;
    static U32Vec decisions, constraints, open, ckinds, cinsts, ckind_of;
    static U32Vec ckind_over; /* per custom-kind section, entries the cap held
                                 back; parallel to ckinds */
    static U32Vec causal;
    static HistEntry *hist;
    static u32 hist_cap;
    u32 hist_count = 0;
    i64 remaining;
    g_sort_g = g;
    hood.count = pool.count = state.count = recent.count = related.count = 0;
    nbr.count = sect.count = consumed.count = causal.count = 0;
    decisions.count = constraints.count = open.count = 0;
    ckinds.count = cinsts.count = ckind_of.count = ckind_over.count = 0;

    /* hood: the one-hop relations of focus — store-wide in orientation
     * mode, where the seed-tick ontology stays out of the walk */
    if (w->orientation) {
      for (i = 0; i < g->relation_count; i++)
        if (g->relations[i].created_at > 0)
          u32vec_push(&hood, i);
    } else {
      for (i = 0; i < w->focus_elems.count; i++) {
        u32 link = g->rels_by_elem[w->focus_elems.v[i]];
        while (link != NONE_U32) {
          u32vec_push_unique(&hood, g->rel_links[link].rel);
          link = g->rel_links[link].next;
        }
      }
      /* A statement that CONTAINS a focused one is part of the answer: focusing
       * `em dash` must reach `Nick asked me [to remove em dashes]`, or the
       * request is invisible from the only term the reader has. Containment
       * only -- a `subject: rel` meta is provenance/modality, already carried by
       * its own section, and lifting those here would drag all 1167 source metas
       * into the neighborhood. Bounded to the depth the frame will actually
       * render, so the walk can never outrun what the reader gets shown. */
      {
        u32 d;
        for (d = 0; d < FRAME_NEST_MAX; d++) {
          u32 grown = hood.count;
          for (i = 0; i < grown; i++) {
            u32 inner = hood.v[i];
            u32 link = g->meta_by_target[inner];
            while (link != NONE_U32) {
              u32 up = g->rel_links[link].rel;
              const Relation *c = &g->relations[up];
              u32 a;
              for (a = 0; a < c->attr_count; a++)
                if (c->attrs[a].value.id == inner && frame_slot_nests(&c->attrs[a]))
                  u32vec_push_unique(&hood, up);
              link = g->rel_links[link].next;
            }
          }
        }
      }
    }
    for (i = 0; i < hood.count; i++)
      u32vec_push(&pool, hood.v[i]);
    for (i = 0; i < w->minted_rels.count; i++)
      u32vec_push_unique(&pool, w->minted_rels.v[i]);
    for (i = 0; i < w->reused_rel_count; i++)
      u32vec_push_unique(&pool, w->reused_rels[i].rel);

    /* neighborhood elements: focus + slots of the live pool (every live
     * element in orientation mode) */
    if (w->orientation) {
      for (i = 0; i < g->element_count; i++)
        if (g->elements[i].redirect == NONE_U32)
          u32vec_push(&nbr, i);
    } else {
      for (i = 0; i < w->focus_elems.count; i++)
        u32vec_push_unique(&nbr, w->focus_elems.v[i]);
      for (i = 0; i < pool.count; i++) {
        const Relation *r = &g->relations[pool.v[i]];
        u32 a;
        if (r->status >= ST_SUPERSEDED || rel_is_meta(g, pool.v[i]))
          continue;
        for (a = 0; a < r->attr_count; a++)
          if (r->attrs[a].value.tag == TERM_ELEM)
            u32vec_push_unique(&nbr, r->attrs[a].value.id);
      }
    }

    /* state: live current_* caches whose subject is in the neighborhood */
    for (i = 0; i < nbr.count; i++) {
      u32 link = g->rels_by_elem[nbr.v[i]];
      while (link != NONE_U32) {
        u32 rid = g->rel_links[link].rel;
        link = g->rel_links[link].next;
        if (g->relations[rid].status < ST_SUPERSEDED &&
            rel_cache_subject(g, rid) == nbr.v[i])
          u32vec_push_unique(&state, rid);
      }
    }
    u32vec_sort(&state, rel_recency_cmp);

    /* kind-section candidates: nbr elements plus subjects of live
     * relations that reference an nbr element — a constraint or open
     * item written in an earlier session surfaces because it links to
     * the focus neighborhood (spec §7) */
    for (i = 0; i < nbr.count; i++)
      u32vec_push(&sect, nbr.v[i]);
    if (!w->orientation) {
      for (i = 0; i < nbr.count; i++) {
        u32 link = g->rels_by_elem[nbr.v[i]];
        while (link != NONE_U32) {
          u32 rid = g->rel_links[link].rel;
          link = g->rel_links[link].next;
          {
            const Relation *r = &g->relations[rid];
            u32 a;
            if (r->status >= ST_SUPERSEDED || rel_is_meta(g, rid))
              continue;
            for (a = 0; a < r->attr_count; a++)
              if (r->attrs[a].name == WK_SUBJECT &&
                  r->attrs[a].value.tag == TERM_ELEM)
                u32vec_push_unique(&sect, r->attrs[a].value.id);
          }
        }
      }
    }

    /* kind sections; their attr relations leave recent/related. Beyond
     * the seeded three, any non-seeded template kind with instances here
     * gets a section keyed by its kind name (spec §7). */
    for (i = 0; i < sect.count; i++) {
      u32 e = sect.v[i], kind = g->elem_kind[e];
      static U32Vec eattrs;
      int pushed = 0;
      if (kind == NONE_U32)
        continue;
      if (kind == WK_KIND_DECISION) {
        u32vec_push(&decisions, e);
        pushed = 1;
      } else if (kind == WK_KIND_CONSTRAINT) {
        if (constraint_is_active(g, e)) {
          u32vec_push(&constraints, e);
          pushed = 1;
        }
      } else if (kind == WK_KIND_QUESTION || kind == WK_KIND_TASK) {
        if (elem_is_open(g, e)) {
          u32vec_push(&open, e);
          pushed = 1;
        }
      } else if (kind >= WK_ELEMENT_COUNT && kind_has_template(g, kind)) {
        u32vec_push_unique(&ckinds, kind);
        u32vec_push(&cinsts, e);
        u32vec_push(&ckind_of, kind);
        pushed = 1;
      }
      /* an inactive constraint or closed question is not emitted in any
       * section, so its still-live relations must stay in recent/related
       * — only a pushed element's attrs are consumed (finding 4) */
      if (!pushed)
        continue;
      collect_instance_attrs(g, e, &eattrs);
      for (j = 0; j < eattrs.count; j++)
        u32vec_push_unique(&consumed, eattrs.v[j]);
    }
    u32vec_sort(&decisions, elem_recency_cmp);
    u32vec_sort(&constraints, elem_recency_cmp);
    u32vec_sort(&open, elem_recency_cmp);
    u32vec_sort(&ckinds, u32_cmp);

    /* causal: caused/enables/prevents/correlated_with edges in the focus
     * neighborhood get a typed section (with rung + modality) and are consumed
     * so they do not also appear as untyped recent/related edges (§16.3). */
    for (i = 0; i < pool.count; i++) {
      u32 rid = pool.v[i];
      if (g->relations[rid].status >= ST_SUPERSEDED || rel_is_meta(g, rid))
        continue;
      if (rel_causal_pred(g, rid) != NONE_U32) {
        u32vec_push_unique(&causal, rid);
        u32vec_push_unique(&consumed, rid);
      }
    }
    u32vec_sort(&causal, rel_recency_cmp);

    /* recent vs related. recent = base relations from the tick's writes
     * (mints) and the focus neighborhood — on a recall the whole
     * neighborhood is recency; a save's frame keeps older neighborhood
     * edges in related (the spec §7 example: rel:512 "fits no typed
     * section and so lands in related") except change events, which stay
     * live in recent forever. A reused write outside the neighborhood
     * surfaces only through writes.reused_relations. */
    for (i = 0; i < pool.count; i++) {
      u32 rid = pool.v[i];
      const Relation *r = &g->relations[rid];
      int in_hood, minted_now;
      if (r->status >= ST_SUPERSEDED)
        continue;
      if (rel_is_meta(g, rid))
        continue;
      if (rel_cache_subject(g, rid) != NONE_U32)
        continue; /* caches live in state */
      if (u32vec_has(&consumed, rid))
        continue;
      in_hood = u32vec_has(&hood, rid);
      minted_now = u32vec_has(&w->minted_rels, rid);
      if (minted_now || (in_hood && (w->is_recall || rel_event_shaped(g, rid))))
        u32vec_push(&recent, rid);
      else if (in_hood)
        u32vec_push(&related, rid);
    }
    u32vec_sort(&recent, rel_recency_cmp);
    if (w->is_recall && w->curiosity > 0.0)
      rank_recent_novelty(g, &recent, w->curiosity);
    rank_related(g, &related);
    /* F1: on a focused recall with embeddings on, rank the neighborhood facts
     * by relevance to the query rather than recency/activation. */
    if (w->is_recall) {
      rerank_relevance(g, &recent, w->focus_query);
      rerank_relevance(g, &related, w->focus_query);
    }

    /* Orientation state is store-wide and capped (spec §6): at most half
     * the limit, newest first, so recent/related keep budget room — a
     * session boot that shows every cache but no recent work fails its one
     * job. Focused frames emit their whole (focus-scoped) state. Capped
     * before the history BFS so hidden caches' chains can't inflate history
     * and eat the recent/related budget (finding 6). */
    if (w->orientation && limit >= 0 && (i64)state.count > limit / 2)
      state.count = (u32)(limit / 2);

    /* history: supersession chains behind each state cache */
    for (i = 0; i < state.count; i++) {
      static U32Vec frontier, next;
      i32 depth = history_depth;
      frontier.count = 0;
      u32vec_push(&frontier, state.v[i]);
      while (depth != 0 && frontier.count) {
        next.count = 0;
        for (j = 0; j < frontier.count; j++) {
          u32 n = frontier.v[j];
          static U32Vec olds, ats;
          u32 link = g->meta_by_target[n];
          u32 k;
          olds.count = ats.count = 0;
          while (link != NONE_U32) {
            u32 mid = g->rel_links[link].rel;
            link = g->rel_links[link].next;
            {
              const Relation *m = &g->relations[mid];
              u32 a, old = NONE_U32;
              int subj_is_n = 0;
              if (m->status == ST_RETRACTED)
                continue;
              for (a = 0; a < m->attr_count; a++) {
                if (m->attrs[a].name == WK_SUBJECT &&
                    m->attrs[a].value.tag == TERM_REL &&
                    m->attrs[a].value.id == n)
                  subj_is_n = 1;
                if (m->attrs[a].name == WK_SUPERSEDES &&
                    m->attrs[a].value.tag == TERM_REL)
                  old = m->attrs[a].value.id;
              }
              if (!subj_is_n || old == NONE_U32)
                continue;
              u32vec_push(&olds, old);
              u32vec_push(&ats, m->created_at);
            }
          }
          /* id desc within one superseder */
          for (k = 0; k < olds.count; k++) {
            u32 best = k, m2;
            u32 tmp;
            for (m2 = k + 1; m2 < olds.count; m2++)
              if (olds.v[m2] > olds.v[best])
                best = m2;
            tmp = olds.v[k];
            olds.v[k] = olds.v[best];
            olds.v[best] = tmp;
            tmp = ats.v[k];
            ats.v[k] = ats.v[best];
            ats.v[best] = tmp;
          }
          for (k = 0; k < olds.count; k++) {
            u32 h;
            int dup = 0;
            for (h = 0; h < hist_count; h++)
              if (hist[h].rel == olds.v[k])
                dup = 1;
            if (dup)
              continue;
            hist = (HistEntry *)xgrow(hist, hist_count + 1, &hist_cap,
                                      sizeof *hist);
            hist[hist_count].rel = olds.v[k];
            hist[hist_count].by = n;
            hist[hist_count].at_tick = ats.v[k];
            hist_count++;
            u32vec_push(&next, olds.v[k]);
          }
        }
        frontier.count = 0;
        for (j = 0; j < next.count; j++)
          u32vec_push(&frontier, next.v[j]);
        if (depth > 0)
          depth--;
      }
    }

    /* focus header — or the orientation overview (pin §3.15) */
    if (w->orientation) {
      printf(",\"overview\":{\"elements\":%u,\"relations\":%u,\"clock\":%u",
             g->element_count, g->relation_count, w->tick);
      frame_put_audit_tally(g);
      fputs(",\"scope\":", stdout);
      {
        u32 scope = NONE_U32;
        for (i = 0; i < g->element_count; i++)
          if (g->elements[i].redirect == NONE_U32 &&
              g->elem_kind[i] == WK_KIND_PROJECT) {
            scope = i;
            break;
          }
        if (scope != NONE_U32)
          frame_put_focus_entry(g, scope);
        else
          fputs("null", stdout);
      }
      fputs(",\"active\":[", stdout);
      {
        /* Eligibility (deterministic, fixing the leaf-value/kind
         * pollution nit): a live non-seed element that is not
         * attribute vocabulary (serving as a slot name anywhere is
         * plumbing, not a topic) AND is a topic object — it carries a
         * summary, or it sits in the subject slot of at least one
         * live relation that is not an `expects` template edge.
         * instance_of typing counts (declaring a kind declares a
         * topic); kind elements themselves never qualify (they are
         * instance_of values, and their expects edges are excluded);
         * leaf values ("4.2", "active") are never subjects. Cap 5. */
        static U32Vec act;
        static u8 *is_aname;
        static u32 is_aname_cap;
        u32 a;
        act.count = 0;
        is_aname =
            (u8 *)xgrow(is_aname, g->element_count ? g->element_count : 1,
                        &is_aname_cap, 1);
        memset(is_aname, 0, g->element_count);
        for (i = 0; i < g->relation_count; i++)
          for (a = 0; a < g->relations[i].attr_count; a++)
            is_aname[g->relations[i].attrs[a].name] = 1;
        for (i = WK_ELEMENT_COUNT; i < g->element_count; i++) {
          int topic = g->elements[i].summary != NONE_U32;
          u32 link;
          if (g->elements[i].redirect != NONE_U32 || is_aname[i])
            continue;
          if (is_core_vocab(g, i)) /* vocabulary is plumbing, never a topic --
                                      the self anchor carries a summary and
                                      subjects facts, so it would otherwise
                                      qualify on both counts */
            continue;
          for (link = g->rels_by_elem[i]; !topic && link != NONE_U32;
               link = g->rel_links[link].next) {
            const Relation *r = &g->relations[g->rel_links[link].rel];
            int subj = 0, expects = 0;
            if (r->status >= ST_SUPERSEDED)
              continue;
            for (a = 0; a < r->attr_count; a++) {
              if (r->attrs[a].name == WK_SUBJECT &&
                  r->attrs[a].value.tag == TERM_ELEM &&
                  r->attrs[a].value.id == i)
                subj = 1;
              if (r->attrs[a].name == WK_EXPECTS)
                expects = 1;
            }
            if (subj && !expects)
              topic = 1;
          }
          if (topic)
            u32vec_push(&act, i);
        }
        u32vec_sort(&act, elem_active_cmp);
        for (i = 0; i < act.count && i < 5; i++) {
          if (i)
            fputc(',', stdout);
          frame_put_focus_entry(g, act.v[i]);
        }
      }
      fputs("]}", stdout);
    } else {
      fputs(",\"focus\":[", stdout);
      for (i = 0; i < w->focus_elems.count; i++) {
        if (i)
          fputc(',', stdout);
        frame_put_focus_entry(g, w->focus_elems.v[i]);
      }
      fputc(']', stdout);
    }
    fputs(",\"state\":[", stdout);
    for (i = 0; i < state.count; i++)
      frame_put_rel_entry(g, state.v[i], i == 0);
    fputs("],\"decisions\":[", stdout);
    for (i = 0; i < decisions.count && i < g_frame_section_cap; i++)
      frame_put_instance(g, decisions.v[i], 0, i == 0);
    fputs("],\"constraints\":[", stdout);
    for (i = 0; i < constraints.count && i < g_frame_section_cap; i++)
      frame_put_instance(g, constraints.v[i], 0, i == 0);
    fputs("],\"open\":[", stdout);
    for (i = 0; i < open.count && i < g_frame_section_cap; i++)
      frame_put_instance(g, open.v[i], 1, i == 0);
    fputc(']', stdout);
    /* one section per non-seeded template kind with instances here,
     * keyed by the kind's name, kind ids ascending (spec §7) */
    for (i = 0; i < ckinds.count; i++) {
      static U32Vec insts;
      insts.count = 0;
      for (j = 0; j < cinsts.count; j++)
        if (ckind_of.v[j] == ckinds.v[i])
          u32vec_push(&insts, cinsts.v[j]);
      u32vec_sort(&insts, elem_recency_cmp);
      fputs(",\"", stdout);
      json_escape_fputs(elem_name(g, ckinds.v[i]), stdout);
      fputs("\":[", stdout);
      for (j = 0; j < insts.count && j < g_frame_section_cap; j++)
        frame_put_instance(g, insts.v[j], 0, j == 0);
      fputc(']', stdout);
      if (insts.count > g_frame_section_cap)
        u32vec_push(&ckind_over, insts.count - g_frame_section_cap);
      else
        u32vec_push(&ckind_over, 0);
    }
    fputs(",\"recent\":[", stdout);
    /* the limit budget (spec §6): state, history, and the typed sections
     * are always included; recent fills newest-first, then related
     * ranked, under what remains */
    {
      u32 hist_emit = 0;
      for (i = 0; i < hist_count; i++)
        if (!(since >= 0 &&
              frame_tick_day(g, g->relations[hist[i].rel].created_at) * 86400 <
                  since))
          hist_emit++;
      remaining = limit < 0 ? -1 : limit - (i64)state.count - (i64)hist_emit;
      if (remaining < 0 && limit >= 0)
        remaining = 0;
    }
    {
      u32 put = 0;
      for (i = 0; i < recent.count; i++) {
        if (since >= 0 &&
            frame_tick_day(g, g->relations[recent.v[i]].created_at) * 86400 <
                since)
          continue;
        if (remaining == 0)
          break;
        frame_put_rel_entry(g, recent.v[i], put == 0);
        put++;
        if (remaining > 0)
          remaining--;
      }
    }
    fputs("],\"history\":[", stdout);
    {
      u32 put = 0;
      for (i = 0; i < hist_count; i++) {
        const Relation *r = &g->relations[hist[i].rel];
        if (since >= 0 && frame_tick_day(g, r->created_at) * 86400 < since)
          continue;
        if (put)
          fputc(',', stdout);
        printf("{\"ref\":\"rel:%u\",\"attrs\":", hist[i].rel);
        frame_put_rel_attrs(g, hist[i].rel);
        printf(",\"asserted_at\":%u,\"asserted\":\"", r->created_at);
        frame_put_day(g, r->created_at);
        printf("\",\"superseded_by\":\"rel:%u\",\"superseded_at\":%u}",
               hist[i].by, hist[i].at_tick);
        put++;
      }
    }
    fputs("],\"related\":[", stdout);
    {
      u32 put = 0;
      for (i = 0; i < related.count; i++) {
        if (remaining == 0)
          break;
        frame_put_rel_entry(g, related.v[i], put == 0);
        put++;
        if (remaining > 0)
          remaining--;
      }
    }
    fputs("],\"pointers\":[", stdout);
    {
      u32 put = 0;
      for (i = 0; i < w->ptr_count; i++) {
        u32 e = w->ptrs[i].elem;
        int seen = 0;
        for (j = 0; j < i; j++)
          if (w->ptrs[j].elem == e)
            seen = 1;
        if (seen)
          continue;
        if (put++)
          fputc(',', stdout);
        fputs("{\"src\":\"", stdout);
        json_escape_fputs(elem_name(g, e), stdout);
        fputs("\",\"rels\":[", stdout);
        {
          u32 put2 = 0;
          for (j = 0; j < w->ptr_count; j++)
            if (w->ptrs[j].elem == e)
              printf("%s\"rel:%u\"", put2++ ? "," : "", w->ptrs[j].rel);
        }
        fputs("]}", stdout);
      }
    }
    fputs("],\"sources\":[", stdout);
    {
      u32 put = 0;
      for (i = 0; i < w->src_count; i++) {
        u32 e = w->srcs[i].elem;
        int seen = 0;
        for (j = 0; j < i; j++)
          if (w->srcs[j].elem == e)
            seen = 1;
        if (seen)
          continue;
        if (put++)
          fputc(',', stdout);
        fputs("{\"source\":\"", stdout);
        json_escape_fputs(elem_name(g, e), stdout);
        fputs("\",\"rels\":[", stdout);
        {
          u32 put2 = 0;
          for (j = 0; j < w->src_count; j++)
            if (w->srcs[j].elem == e)
              printf("%s\"rel:%u\"", put2++ ? "," : "", w->srcs[j].rel);
        }
        fputs("]}", stdout);
      }
    }
    fputs("],\"causal\":[", stdout);
    for (i = 0; i < causal.count && i < g_frame_section_cap; i++)
      frame_put_causal_entry(g, causal.v[i], i == 0);
    fputs("]", stdout);
    /* What the caps held back, so a short section never reads as a complete
     * one. Emitted only when something was actually dropped. */
    {
      u32 over[4], put = 0;
      static const char *const NAMES[4] = {"decisions", "constraints", "open",
                                           "causal"};
      over[0] = decisions.count > g_frame_section_cap
                    ? decisions.count - g_frame_section_cap : 0;
      over[1] = constraints.count > g_frame_section_cap
                    ? constraints.count - g_frame_section_cap : 0;
      over[2] = open.count > g_frame_section_cap
                    ? open.count - g_frame_section_cap : 0;
      over[3] = causal.count > g_frame_section_cap
                    ? causal.count - g_frame_section_cap : 0;
      for (i = 0; i < 4; i++)
        put += over[i] != 0;
      for (i = 0; i < ckind_over.count; i++)
        put += ckind_over.v[i] != 0;
      if (put) {
        u32 first = 1;
        fputs(",\"omitted\":{", stdout);
        for (i = 0; i < 4; i++)
          if (over[i]) {
            printf("%s\"%s\":%u", first ? "" : ",", NAMES[i], over[i]);
            first = 0;
          }
        for (i = 0; i < ckind_over.count; i++)
          if (ckind_over.v[i]) {
            fputs(first ? "\"" : ",\"", stdout);
            json_escape_fputs(elem_name(g, ckinds.v[i]), stdout);
            printf("\":%u", ckind_over.v[i]);
            first = 0;
          }
        fputc('}', stdout);
      }
    }
  }
  fputs("}\n", stdout);
}

/* ============================ S9 — persistence ============================ */
/*
 * How the store survives between invocations, and how it is read back safely.
 *
 * WRITE: serialize the whole live graph to a byte buffer and write it with the
 * durable dance tmp -> fsync(file) -> rename -> fsync(dir) (spec §11). The
 * rename is the commit point: a reader either sees the whole old snapshot or
 * the whole new one, never a half-written file, even across a power cut. Every
 * value is written by an explicit little-endian byte helper (bb_u32/bb_u64/
 * bb_f64) — NO struct is ever fwritten and no float ever goes through libc
 * text I/O, so the on-disk format is a fixed byte-layout table the Rust build
 * reproduces independently.
 *
 * READ: "a snapshot is input, and input is hostile." A snapshot file survives
 * git checkouts, hand edits, power-loss partial states, and (eventually) merge
 * drivers reading a foreign blob. So the reader (SnapRd, sr_*) treats it as
 * untrusted: every count and length is checked against the bytes remaining
 * BEFORE any allocation, every stored u32 handle is range-checked as the graph
 * rebuilds, and every f64 is checked finite. Any violation is a clean
 * `snapshot_corrupt` error via fail() — never a crash, never undefined
 * behavior. The corrupt-snapshot fuzz target hammers exactly this path.
 *
 * Derived indices are never stored; they are recomputed on load, so a bad
 * index can never be smuggled in.
 *
 * Snapshot byte layout, version 1 (frozen at M1 against plan §8's checklist:
 * redirect tombstones, full status lattice, support_diversity + recorded
 * supporter sources, stability/salience/activation stats, policy block,
 * stamp table). The Rust build implements this table independently.
 *
 * All multi-byte integers are little-endian. f64 is the IEEE-754 bit pattern
 * carried as u64le — no struct is ever fwritten, no libc float text I/O.
 * NONE (0xFFFFFFFF) marks absent u32 handles.
 *
 *   magic            8 bytes  "LGND3SNP"
 *   version          u32      1
 *   file_length      u64      declared total; must equal the file size
 *   clock            u32      ticks so far
 *   policy           11 f64:  hebbian_rate, decay_rate, stability_spaced,
 *                             stability_massed, stability_cap,
 *                             default_confidence, supersession_threshold,
 *                             salience_conflict, salience_supersession,
 *                             salience_novel, salience_reuse_bump
 *                    3 u32:   focus_decay_radius, promotion_min_support,
 *                             promotion_min_diversity
 *   stamp_count      u32      must equal clock
 *   stamps           stamp_count x u64   wall seconds of tick i+1
 *   string_count     u32
 *   strings          string_count x { len u32, bytes len }   id = record index
 *   element_count    u32
 *   elements         element_count x {
 *     name_count     u32      >= 1; names[0] is canonical
 *     names          name_count x u32   string ids
 *     summary        u32      string id or NONE
 *     redirect       u32      element id or NONE; a redirect's target is live
 *                             (single hop — the reader rejects chains)
 *     created_at     u32      <= clock
 *     stats          4 f64 (confidence, activation, salience, stability) then
 *                    5 u32 (access_count, focus_success_count, support_count,
 *                           support_diversity, last_seen <= clock)
 *   }
 *   relation_count   u32
 *   relations        relation_count x {
 *     attr_count     u8       2..5
 *     status         u8       0 asserted, 1 entailed, 2 defeasible,
 *                             3 superseded, 4 retracted
 *     attrs          attr_count x { name u32 (element id), tag u8 (0 element,
 *                                   1 relation), id u32 (range per tag) }
 *     created_at     u32      <= clock
 *     stats          as element stats
 *     supporter_count u32
 *     supporters     supporter_count x u32   source element ids
 *   }
 *   (end: no trailing bytes)
 *
 * Derived indices are never persisted; the reader rebuilds them and verifies
 * the S4 well-known ontology ids on the way (elements 0..31 by name).
 */

#define SNAP_MAGIC "LGND3SNP"
enum { SNAP_VERSION = 1 };

/* ---- byte writer ---- */

typedef struct {
  u8 *v;
  u32 len, cap;
} ByteBuf;

static void bb_bytes(ByteBuf *b, const void *p, u32 n) {
  b->v = (u8 *)xgrow(b->v, b->len + n, &b->cap, 1);
  memcpy(b->v + b->len, p, n);
  b->len += n;
}

static void bb_u8(ByteBuf *b, u8 x) { bb_bytes(b, &x, 1); }

static void bb_u32(ByteBuf *b, u32 x) {
  u8 t[4];
  t[0] = (u8)x;
  t[1] = (u8)(x >> 8);
  t[2] = (u8)(x >> 16);
  t[3] = (u8)(x >> 24);
  bb_bytes(b, t, 4);
}

static void bb_u64(ByteBuf *b, u64 x) {
  u8 t[8];
  int i;
  for (i = 0; i < 8; i++)
    t[i] = (u8)(x >> (8 * i));
  bb_bytes(b, t, 8);
}

static void bb_f64(ByteBuf *b, double x) {
  u64 bits;
  memcpy(&bits, &x, 8);
  bb_u64(b, bits);
}

static void bb_stats(ByteBuf *b, const Stats *s) {
  bb_f64(b, s->confidence);
  bb_f64(b, s->activation);
  bb_f64(b, s->salience);
  bb_f64(b, s->stability);
  bb_u32(b, s->access_count);
  bb_u32(b, s->focus_success_count);
  bb_u32(b, s->support_count);
  bb_u32(b, s->support_diversity);
  bb_u32(b, s->last_seen);
}

static void snapshot_serialize(const Hypergraph *g, ByteBuf *b) {
  u32 i, k;
  b->len = 0;
  bb_bytes(b, SNAP_MAGIC, 8);
  bb_u32(b, SNAP_VERSION);
  bb_u64(b, 0); /* file_length, patched below */
  bb_u32(b, g->clock);
  bb_f64(b, g->policy.hebbian_rate);
  bb_f64(b, g->policy.decay_rate);
  bb_f64(b, g->policy.stability_spaced);
  bb_f64(b, g->policy.stability_massed);
  bb_f64(b, g->policy.stability_cap);
  bb_f64(b, g->policy.default_confidence);
  bb_f64(b, g->policy.supersession_threshold);
  bb_f64(b, g->policy.salience_conflict);
  bb_f64(b, g->policy.salience_supersession);
  bb_f64(b, g->policy.salience_novel);
  bb_f64(b, g->policy.salience_reuse_bump);
  bb_u32(b, g->policy.focus_decay_radius);
  bb_u32(b, g->policy.promotion_min_support);
  bb_u32(b, g->policy.promotion_min_diversity);
  bb_u32(b, g->clock); /* stamp_count == clock */
  for (i = 0; i < g->clock; i++)
    bb_u64(b, g->stamps[i]);
  bb_u32(b, g->strs.count);
  for (i = 0; i < g->strs.count; i++) {
    bb_u32(b, str_len(&g->strs, i));
    bb_bytes(b, str_ptr(&g->strs, i), str_len(&g->strs, i));
  }
  bb_u32(b, g->element_count);
  for (i = 0; i < g->element_count; i++) {
    const Element *e = &g->elements[i];
    bb_u32(b, e->names.count);
    for (k = 0; k < e->names.count; k++)
      bb_u32(b, e->names.v[k]);
    bb_u32(b, e->summary);
    bb_u32(b, e->redirect);
    bb_u32(b, e->created_at);
    bb_stats(b, &e->stats);
  }
  bb_u32(b, g->relation_count);
  for (i = 0; i < g->relation_count; i++) {
    const Relation *r = &g->relations[i];
    bb_u8(b, r->attr_count);
    bb_u8(b, r->status);
    for (k = 0; k < r->attr_count; k++) {
      bb_u32(b, r->attrs[k].name);
      bb_u8(b, r->attrs[k].value.tag);
      bb_u32(b, r->attrs[k].value.id);
    }
    bb_u32(b, r->created_at);
    bb_stats(b, &r->stats);
    bb_u32(b, r->supporters.count);
    for (k = 0; k < r->supporters.count; k++)
      bb_u32(b, r->supporters.v[k]);
  }
  /* patch the declared length */
  for (i = 0; i < 8; i++)
    b->v[12 + i] = (u8)((u64)b->len >> (8 * i));
}

/* tmp + fsync(file) + rename + fsync(dir) (spec §11). Failures surface as
 * no_store: there is no io error code and the store is what went bad. */
static void snapshot_write(const Hypergraph *g, const char *store) {
  static ByteBuf b; /* static: reachable if a trapped fail() longjmps */
  char tmp[4400], final_path[4400];
  int fd;
  u32 off = 0;
  snapshot_serialize(g, &b);
  snprintf(tmp, sizeof tmp, "%s/legend.snapshot.tmp", store);
  snprintf(final_path, sizeof final_path, "%s/legend.snapshot", store);
  fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0666);
  if (fd < 0)
    fail(ERR_NO_STORE, NULL, "cannot write snapshot %s: %s", tmp,
         strerror(errno));
  while (off < b.len) {
    ssize_t n = write(fd, b.v + off, b.len - off);
    if (n < 0) {
      if (errno == EINTR)
        continue;
      close(fd);
      fail(ERR_NO_STORE, NULL, "cannot write snapshot %s: %s", tmp,
           strerror(errno));
    }
    off += (u32)n;
  }
  if (fsync(fd) != 0) {
    close(fd);
    fail(ERR_NO_STORE, NULL, "cannot fsync snapshot %s: %s", tmp,
         strerror(errno));
  }
  close(fd);
  if (rename(tmp, final_path) != 0)
    fail(ERR_NO_STORE, NULL, "cannot rename snapshot into place: %s",
         strerror(errno));
  fd = open(store, O_RDONLY | O_CLOEXEC);
  if (fd >= 0) {
    fsync(fd);
    close(fd);
  } /* best-effort directory durability */
}

/* Orphan .tmp sweep, run under the lock at load. Skips legend.lock by name
 * (plan §2 iron rule 3 keeps the lock fd unique; this keeps the file safe). */
static void sweep_orphan_tmps(const char *store) {
  DIR *d = opendir(store);
  struct dirent *ent;
  if (!d)
    return;
  while ((ent = readdir(d)) != NULL) {
    char p[4500];
    if (strcmp(ent->d_name, "legend.lock") == 0)
      continue;
    if (strncmp(ent->d_name, "legend.snapshot.tmp", 19) != 0)
      continue;
    snprintf(p, sizeof p, "%s/%s", store, ent->d_name);
    unlink(p);
  }
  closedir(d);
}

/* ---- validating reader (plan §3.11) ----
 * Every count and length is checked against the remaining bytes before any
 * allocation; every stored handle is range-checked during rebuild; any
 * violation is a clean snapshot_corrupt, never UB. */

typedef struct {
  const u8 *p;
  u64 len, pos;
} SnapRd;

static void sr_need(SnapRd *r, u64 n) {
  if (r->len - r->pos < n)
    fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot truncated at byte %llu",
         (unsigned long long)r->pos);
}

static u8 sr_u8(SnapRd *r) {
  sr_need(r, 1);
  return r->p[r->pos++];
}

static u32 sr_u32(SnapRd *r) {
  u32 v;
  sr_need(r, 4);
  v = (u32)r->p[r->pos] | (u32)r->p[r->pos + 1] << 8 |
      (u32)r->p[r->pos + 2] << 16 | (u32)r->p[r->pos + 3] << 24;
  r->pos += 4;
  return v;
}

static u64 sr_u64(SnapRd *r) {
  u64 v = 0;
  int i;
  sr_need(r, 8);
  for (i = 0; i < 8; i++)
    v |= (u64)r->p[r->pos + i] << (8 * i);
  r->pos += 8;
  return v;
}

/* Finite iff x - x == 0 under strict IEEE (NaN and +-inf both yield NaN). */
static int f64_is_finite(double x) { return x - x == 0.0; }

static double sr_f64(SnapRd *r, const char *what) {
  u64 bits = sr_u64(r);
  double x;
  memcpy(&x, &bits, 8);
  if (!f64_is_finite(x))
    fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot holds a non-finite %s", what);
  return x;
}

static void sr_stats(SnapRd *r, Stats *s, u32 clock) {
  s->confidence = sr_f64(r, "confidence");
  s->activation = sr_f64(r, "activation");
  s->salience = sr_f64(r, "salience");
  s->stability = sr_f64(r, "stability");
  s->access_count = sr_u32(r);
  s->focus_success_count = sr_u32(r);
  s->support_count = sr_u32(r);
  s->support_diversity = sr_u32(r);
  s->last_seen = sr_u32(r);
  if (s->last_seen > clock)
    fail(ERR_SNAPSHOT_CORRUPT, NULL,
         "snapshot stat last_seen %u exceeds clock %u", s->last_seen, clock);
}

static u8 *g_snap_buf; /* static: reachable when a trapped fail() longjmps */

/* Load the store's snapshot into an empty graph and rebuild the derived
 * indices. Returns 1 on success, 0 when no snapshot exists; anything invalid
 * is snapshot_corrupt via fail(). */
static int snapshot_load(Hypergraph *g, const char *store) {
  char path[4400];
  int fd;
  struct stat st;
  SnapRd r;
  u32 clock, string_count, element_count, relation_count, i, k;

  snprintf(path, sizeof path, "%s/legend.snapshot", store);
  fd = open(path, O_RDONLY | O_CLOEXEC);
  if (fd < 0) {
    if (errno == ENOENT)
      return 0;
    fail(ERR_NO_STORE, NULL, "cannot open snapshot %s: %s", path,
         strerror(errno));
  }
  if (fstat(fd, &st) != 0 || st.st_size < 0) {
    close(fd);
    fail(ERR_NO_STORE, NULL, "cannot stat snapshot %s", path);
  }
  free(g_snap_buf);
  g_snap_buf = (u8 *)malloc(st.st_size ? (size_t)st.st_size : 1);
  if (!g_snap_buf) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  {
    u64 off = 0;
    while (off < (u64)st.st_size) {
      ssize_t n = read(fd, g_snap_buf + off, (size_t)((u64)st.st_size - off));
      if (n < 0 && errno == EINTR)
        continue;
      if (n <= 0) {
        close(fd);
        fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot shrank while reading");
      }
      off += (u64)n;
    }
  }
  close(fd);
  r.p = g_snap_buf;
  r.len = (u64)st.st_size;
  r.pos = 0;

  sr_need(&r, 8);
  if (memcmp(r.p, SNAP_MAGIC, 8) != 0)
    fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot has a bad magic");
  r.pos = 8;
  if (sr_u32(&r) != SNAP_VERSION)
    fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot version is not %d",
         SNAP_VERSION);
  if (sr_u64(&r) != r.len)
    fail(ERR_SNAPSHOT_CORRUPT, NULL,
         "snapshot declared length does not match its size");

  clock = sr_u32(&r);
  g->clock = clock;
  g->policy.hebbian_rate = sr_f64(&r, "policy value");
  g->policy.decay_rate = sr_f64(&r, "policy value");
  g->policy.stability_spaced = sr_f64(&r, "policy value");
  g->policy.stability_massed = sr_f64(&r, "policy value");
  g->policy.stability_cap = sr_f64(&r, "policy value");
  g->policy.default_confidence = sr_f64(&r, "policy value");
  g->policy.supersession_threshold = sr_f64(&r, "policy value");
  g->policy.salience_conflict = sr_f64(&r, "policy value");
  g->policy.salience_supersession = sr_f64(&r, "policy value");
  g->policy.salience_novel = sr_f64(&r, "policy value");
  g->policy.salience_reuse_bump = sr_f64(&r, "policy value");
  g->policy.focus_decay_radius = sr_u32(&r);
  g->policy.promotion_min_support = sr_u32(&r);
  g->policy.promotion_min_diversity = sr_u32(&r);

  if (sr_u32(&r) != clock)
    fail(ERR_SNAPSHOT_CORRUPT, NULL,
         "snapshot stamp table does not match the clock");
  sr_need(&r, (u64)clock * 8);
  g->stamps = (u64 *)xgrow(g->stamps, clock ? clock : 1, &g->stamp_cap,
                           sizeof *g->stamps);
  for (i = 0; i < clock; i++)
    g->stamps[i] = sr_u64(&r);

  string_count = sr_u32(&r);
  if ((u64)string_count * 4 > r.len - r.pos)
    fail(ERR_SNAPSHOT_CORRUPT, NULL,
         "snapshot string count %u exceeds the file", string_count);
  for (i = 0; i < string_count; i++) {
    u32 len = sr_u32(&r);
    sr_need(&r, len);
    if (!utf8_valid((const char *)r.p + r.pos, len))
      fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot string %u is not valid UTF-8",
           i);
    if (str_intern(&g->strs, (const char *)r.p + r.pos, len) != i)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot string table holds a duplicate");
    r.pos += len;
  }

  element_count = sr_u32(&r);
  if ((u64)element_count * 72 > r.len - r.pos) /* min encoded element */
    fail(ERR_SNAPSHOT_CORRUPT, NULL,
         "snapshot element count %u exceeds the file", element_count);
  for (i = 0; i < element_count; i++) {
    u32 id = element_arena_push(g);
    Element *e = &g->elements[id];
    u32 name_count = sr_u32(&r);
    if (name_count < 1 || (u64)name_count * 4 > r.len - r.pos)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot element %u has a bad name count", i);
    for (k = 0; k < name_count; k++) {
      u32 sid = sr_u32(&r);
      if (sid >= string_count)
        fail(ERR_SNAPSHOT_CORRUPT, NULL,
             "snapshot element %u names string %u out of range", i, sid);
      u32vec_push(&e->names, sid);
    }
    e->summary = sr_u32(&r);
    if (e->summary != NONE_U32 && e->summary >= string_count)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot element %u summary out of range", i);
    e->redirect = sr_u32(&r);
    if (e->redirect != NONE_U32 && e->redirect >= element_count)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot element %u redirect out of range", i);
    e->created_at = sr_u32(&r);
    if (e->created_at > clock)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot element %u created after the clock", i);
    sr_stats(&r, &e->stats, clock);
  }
  for (i = 0; i < element_count; i++) {
    u32 rd = g->elements[i].redirect;
    if (rd != NONE_U32 && g->elements[rd].redirect != NONE_U32)
      fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot element %u redirect chains",
           i);
  }

  relation_count = sr_u32(&r);
  if ((u64)relation_count * 80 > r.len - r.pos) /* min encoded relation */
    fail(ERR_SNAPSHOT_CORRUPT, NULL,
         "snapshot relation count %u exceeds the file", relation_count);
  for (i = 0; i < relation_count; i++) {
    u32 id = relation_arena_push(g);
    Relation *rel = &g->relations[id];
    u32 supporter_count;
    rel->attr_count = sr_u8(&r);
    if (rel->attr_count < 2 || rel->attr_count > 5)
      fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot relation %u has %u slots", i,
           rel->attr_count);
    rel->status = sr_u8(&r);
    if (rel->status > ST_RETRACTED)
      fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot relation %u has status %u", i,
           rel->status);
    for (k = 0; k < rel->attr_count; k++) {
      rel->attrs[k].name = sr_u32(&r);
      if (rel->attrs[k].name >= element_count)
        fail(ERR_SNAPSHOT_CORRUPT, NULL,
             "snapshot relation %u slot name out of range", i);
      rel->attrs[k].value.tag = sr_u8(&r);
      if (rel->attrs[k].value.tag > TERM_REL)
        fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot relation %u has term tag %u",
             i, rel->attrs[k].value.tag);
      rel->attrs[k].value.id = sr_u32(&r);
      if (rel->attrs[k].value.id >= (rel->attrs[k].value.tag == TERM_ELEM
                                         ? element_count
                                         : relation_count))
        fail(ERR_SNAPSHOT_CORRUPT, NULL,
             "snapshot relation %u slot value out of range", i);
    }
    rel->created_at = sr_u32(&r);
    if (rel->created_at > clock)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot relation %u created after the clock", i);
    sr_stats(&r, &rel->stats, clock);
    supporter_count = sr_u32(&r);
    if ((u64)supporter_count * 4 > r.len - r.pos)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot relation %u supporter count exceeds the file", i);
    for (k = 0; k < supporter_count; k++) {
      u32 s = sr_u32(&r);
      if (s >= element_count)
        fail(ERR_SNAPSHOT_CORRUPT, NULL,
             "snapshot relation %u supporter out of range", i);
      u32vec_push(&rel->supporters, s);
    }
  }
  if (r.pos != r.len)
    fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot has %llu trailing bytes",
         (unsigned long long)(r.len - r.pos));

  /* the well-known ontology contract (S4) must hold in any v1 snapshot */
  if (element_count < WK_ELEMENT_COUNT)
    fail(ERR_SNAPSHOT_CORRUPT, NULL, "snapshot is missing the core ontology");
  for (i = 0; i < WK_ELEMENT_COUNT; i++) {
    u32 sid = g->elements[i].names.v[0];
    if (str_len(&g->strs, sid) != strlen(WK_NAMES[i]) ||
        memcmp(str_ptr(&g->strs, sid), WK_NAMES[i], str_len(&g->strs, sid)) !=
            0)
      fail(ERR_SNAPSHOT_CORRUPT, NULL,
           "snapshot ontology element #%u is not %s", i, WK_NAMES[i]);
  }

  free(g_snap_buf);
  g_snap_buf = NULL;

  /* every stored name must survive normalization before the rebuild */
  for (i = 0; i < element_count; i++) {
    const Element *e = &g->elements[i];
    for (k = 0; k < e->names.count; k++) {
      if (normalize_into_scratch(str_ptr(&g->strs, e->names.v[k]),
                                 str_len(&g->strs, e->names.v[k])) == 0)
        fail(ERR_SNAPSHOT_CORRUPT, NULL,
             "snapshot element %u name normalizes to nothing", i);
    }
  }
  rebuild_indices(g);
  seed_ext_vocab(g); /* older stores gain the extended vocabulary on open */
  seed_self(g);      /* ...and the self anchor */
  return 1;
}

/* =============================== S10 — cli ===============================
 *
 * The outermost layer: argv parsing, store discovery, locking, and the verb
 * dispatch that stitches S2 (read) -> S6 (tick) -> S9 (save) -> S8 (print)
 * into one invocation. run_cli() at the bottom is the concrete life-of-an-
 * invocation the file header sketched.
 *
 * The ORDER of steps is load-bearing:
 *   1. read the payload (stdin or argv sugar) FULLY, BEFORE taking the lock —
 *      never hold the store's lock while blocked on a caller's pipe.
 *   2. acquire_lock(): one exclusive flock held for the rest of the call, so
 *      concurrent callers block briefly instead of racing. flock (not fcntl
 *      record locks) is chosen so the lock is testable in-process (plan §3.12),
 *      and legend.lock is opened in exactly one place (iron rule 3).
 *   3. parse -> load snapshot -> tick (plan/apply) -> save snapshot -> print.
 *      A recall with "observe":true skips the save (and the tick's clock/
 *      reinforcement), so measurement never trains the store.
 *
 * STDIN DISCIPLINE: read into a fixed 64 KiB+1 buffer; the "+1" byte is how an
 * oversize payload is detected — bufcap bytes arriving means the input was too
 * big, reported as limit_exceeded BEFORE any parsing. The binary is EOF-
 * delimited (one process per payload; the harness closes the pipe).
 *
 * --pretty renders the human surface WITHOUT a second serializer: it captures
 * the JSON frame and re-reads it through the S2 tokenizer, so the two output
 * forms can never drift.
 *
 * main() is guarded by LEGEND_NO_MAIN so the unit tests can #include this whole
 * file and call the internals directly (the stb/SQLite single-file pattern).
 */

static char g_payload[LEGEND_PAYLOAD_CAP +
                      2]; /* cap + 1 overflow-detection byte + NUL */

/* Pristine copy of the payload as submitted: the tokenizer unescapes strings
 * IN PLACE in g_payload, so the journal keeps the original bytes. */
static char g_payload_orig[LEGEND_PAYLOAD_CAP + 2];

/* ---- invocation journal (sidecar diagnostics) ----
 * One JSONL line per invocation appended to <store>/journal.jsonl: the wall
 * time the tick stamped, the build that ran, the verb, and the submitted
 * payload verbatim; ok:false lines carry the error code. Over a long
 * deployment the journal is the record of what callers ACTUALLY submitted —
 * a replayable corpus, a determinism check (replaying it from init must
 * reproduce the live snapshot byte-for-byte), and the rejection log. It never
 * touches frames or snapshots, and a journal write failure never fails the
 * invocation. Armed once the store is known and locked; disarmed by the one
 * append so no invocation writes twice. */
static char g_journal_dir[4300];
static const char *g_journal_verb = "";
static const char *g_journal_payload; /* pristine bytes; NULL = no payload */
static u32 g_journal_payload_len;
static int g_journal_observe;
static int g_journal_armed;
static i64 g_journal_bytes_out = -1; /* recall frame bytes; <0 = not recorded */

static void journal_arm(const char *store, const char *verb,
                        const char *payload, u32 len) {
  snprintf(g_journal_dir, sizeof g_journal_dir, "%s", store);
  g_journal_verb = verb;
  g_journal_payload = payload;
  g_journal_payload_len = len;
  g_journal_observe = 0;
  g_journal_bytes_out = -1;
  g_journal_armed = 1;
}

static void journal_append(i64 ts, int ok, int errcode) {
  char path[4400];
  FILE *f;
  if (!g_journal_armed)
    return;
  g_journal_armed = 0;
  snprintf(path, sizeof path, "%s/journal.jsonl", g_journal_dir);
  f = fopen(path, "a");
  if (!f)
    return;
  fprintf(f, "{\"ts\":%lld,\"build\":\"" LEGEND_BUILD "\",\"verb\":\"%s\"",
          (long long)ts, g_journal_verb);
  if (g_journal_observe)
    fputs(",\"observe\":true", f);
  fprintf(f, ",\"ok\":%s", ok ? "true" : "false");
  if (!ok)
    fprintf(f, ",\"code\":\"%s\"", ERR_CODE_NAMES[errcode]);
  if (g_journal_bytes_out >= 0)
    fprintf(f, ",\"bytes_out\":%lld", (long long)g_journal_bytes_out);
  if (g_journal_payload) {
    fputs(",\"payload\":\"", f);
    json_escape_fwrite(g_journal_payload, g_journal_payload_len, f);
    fputc('"', f);
  }
  fputs("}\n", f);
  fclose(f);
}

/* fail()'s journal line. Real wall clock, not now_unix_seconds(): a malformed
 * LEGEND_NOW reaches here THROUGH fail(), and no tick was stamped anyway. */
static void journal_fail(void) { journal_append((i64)time(NULL), 0, g_err.code); }

/* fread to EOF, at most bufcap bytes (plan §3.13: the binary is EOF-delimited;
 * the harness spawns one process per payload and closes the pipe). The caller
 * classifies bufcap bytes arriving as limit_exceeded before any parsing. */
static u32 read_payload_stream(FILE *f, char *buf, u32 bufcap) {
  u32 total = 0;
  size_t r;
  while (total < bufcap && (r = fread(buf + total, 1, bufcap - total, f)) > 0)
    total += (u32)r;
  return total;
}

static u32 payload_from_stdin(void) {
  u32 n = read_payload_stream(stdin, g_payload, LEGEND_PAYLOAD_CAP + 1);
  if (n > LEGEND_PAYLOAD_CAP)
    fail(ERR_LIMIT_EXCEEDED, NULL, "payload exceeds %d KiB",
         LEGEND_PAYLOAD_CAP / 1024);
  if (n == 0)
    fail(ERR_PARSE, NULL, "empty payload");
  g_payload[n] = 0;
  memcpy(g_payload_orig, g_payload, n + 1);
  return n;
}

/* argv payload sugar runs through the same cap (plan §3.13). */
static u32 payload_from_arg(const char *arg) {
  size_t n = strlen(arg);
  if (n > LEGEND_PAYLOAD_CAP)
    fail(ERR_LIMIT_EXCEEDED, NULL, "payload exceeds %d KiB",
         LEGEND_PAYLOAD_CAP / 1024);
  if (n == 0)
    fail(ERR_PARSE, NULL, "empty payload");
  memcpy(g_payload, arg, n + 1);
  memcpy(g_payload_orig, arg, n + 1);
  return (u32)n;
}

static int dir_exists(const char *path) {
  struct stat st;
  return stat(path, &st) == 0 && S_ISDIR(st.st_mode);
}

/* Store discovery (spec §4): LEGEND_STATE_DIR if set, else walk up from the
 * cwd to the nearest .legend/ directory. Returns 0 when no store exists —
 * only `legend init` creates one. */
static int discover_store(char *out, size_t out_cap) {
  const char *env = getenv("LEGEND_STATE_DIR");
  char cwd[4096];
  if (env && *env) {
    if (!dir_exists(env))
      return 0;
    snprintf(out, out_cap, "%s", env);
    return 1;
  }
  if (!getcwd(cwd, sizeof cwd))
    return 0;
  for (;;) {
    char *slash;
    snprintf(out, out_cap, "%s/.legend", cwd);
    if (dir_exists(out))
      return 1;
    slash = strrchr(cwd, '/');
    if (!slash)
      return 0;
    if (slash == cwd) {
      if (cwd[1] == 0)
        return 0; /* already at "/" */
      cwd[1] = 0;
    } else {
      *slash = 0;
    }
  }
}

static int g_lock_fd = -1;

static i64 mono_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (i64)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

/* flock(2), not fcntl record locks — fcntl releases on any fd close and cannot
 * conflict intra-process, which would make the lock untestable (plan §3.12).
 * This is the only place legend.lock is ever opened (plan §2 iron rule 3). */
static void acquire_lock(const char *store) {
  char path[4300];
  i64 deadline;
  snprintf(path, sizeof path, "%s/legend.lock", store);
  g_lock_fd = open(path, O_RDWR | O_CREAT | O_CLOEXEC, 0666);
  if (g_lock_fd < 0)
    fail(ERR_NO_STORE, NULL, "cannot open %s: %s", path, strerror(errno));
  deadline = mono_ms() + LEGEND_LOCK_TIMEOUT_MS;
  while (flock(g_lock_fd, LOCK_EX | LOCK_NB) != 0) {
    struct timespec nap = {0, 20 * 1000 * 1000};
    if (errno != EWOULDBLOCK && errno != EINTR)
      fail(ERR_LOCK_TIMEOUT, NULL, "flock on %s failed: %s", path,
           strerror(errno));
    if (mono_ms() >= deadline)
      fail(ERR_LOCK_TIMEOUT, NULL,
           "store is locked by another process (waited %d ms)",
           LEGEND_LOCK_TIMEOUT_MS);
    nanosleep(&nap, NULL);
  }
}

/* Release the store lock. The CLI never needs this (process exit drops the
 * flock), but the long-lived mcp-serve holds the lock only per tools/call so it
 * coexists with other legend processes (e.g. a SessionStart hook). */
static void release_lock(void) {
  if (g_lock_fd >= 0) {
    close(g_lock_fd); /* closing the fd releases the flock */
    g_lock_fd = -1;
  }
}

static Hypergraph
    g_graph; /* static: reachable when a trapped fail() longjmps */

/* ---- --pretty: the human surface (spec §4) ----
 * The JSON writer stays the only frame serializer: pretty mode captures the
 * frame bytes and re-reads them through the S2 tokenizer, so the two
 * surfaces cannot drift. Sections render as headed lists. */

static void pretty_bytes(const Json *j, u32 i, FILE *f) {
  fprintf(f, "%.*s", (int)(j->toks[i].end - j->toks[i].start),
          j->buf + j->toks[i].start);
}

/* One-line rendering: objects as k=v pairs, arrays bracketed. */
static void pretty_compact(const Json *j, u32 i, FILE *f) {
  const Tok *t = &j->toks[i];
  u32 k, ti;
  if (t->type == J_OBJ) {
    ti = i + 1;
    for (k = 0; k < t->size; k++) {
      u32 key = ti, val = ti + 1;
      if (k)
        fputs("  ", f);
      pretty_bytes(j, key, f);
      fputc('=', f);
      pretty_compact(j, val, f);
      ti = tok_skip(j->toks, val);
    }
  } else if (t->type == J_ARR) {
    fputc('[', f);
    ti = i + 1;
    for (k = 0; k < t->size; k++) {
      if (k)
        fputs(", ", f);
      pretty_compact(j, ti, f);
      ti = tok_skip(j->toks, ti);
    }
    fputc(']', f);
  } else {
    pretty_bytes(j, i, f);
  }
}

/* The whole frame (either shape): scalars on the header line, one headed
 * list per non-empty section, empty sections skipped. */
static void pretty_render(const Json *j) {
  u32 n = j->toks[0].size, ti = 1, k;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    const Tok *v = &j->toks[val];
    if (v->type == J_OBJ) {
      u32 m = v->size, si = val + 1, p;
      printf("%.*s:\n", (int)(j->toks[key].end - j->toks[key].start),
             j->buf + j->toks[key].start);
      for (p = 0; p < m; p++) {
        u32 skey = si, sval = si + 1;
        if (!(j->toks[sval].type == J_ARR && j->toks[sval].size == 0)) {
          fputs("  ", stdout);
          pretty_bytes(j, skey, stdout);
          fputs(": ", stdout);
          pretty_compact(j, sval, stdout);
          fputc('\n', stdout);
        }
        si = tok_skip(j->toks, sval);
      }
    } else if (v->type == J_ARR) {
      if (v->size > 0) {
        u32 ei = val + 1, p;
        printf("%.*s (%u):\n", (int)(j->toks[key].end - j->toks[key].start),
               j->buf + j->toks[key].start, v->size);
        for (p = 0; p < v->size; p++) {
          fputs("  - ", stdout);
          pretty_compact(j, ei, stdout);
          fputc('\n', stdout);
          ei = tok_skip(j->toks, ei);
        }
      }
    } else {
      pretty_bytes(j, key, stdout);
      fputs(": ", stdout);
      pretty_bytes(j, val, stdout);
      fputc('\n', stdout);
    }
    ti = tok_skip(j->toks, val);
  }
}

/* print_frame with stdout captured into a malloc'd NUL-terminated buffer;
 * dup2 over a tmpfile keeps every frame printer untouched. */
static char *capture_frame_json(const Hypergraph *g, const WriteReport *w,
                                const char *store, i64 limit, i32 history_depth,
                                i64 since, u32 *out_len) {
  FILE *tmp = tmpfile();
  int saved;
  long n;
  char *buf;
  if (!tmp)
    fail(ERR_NO_STORE, NULL, "cannot stage --pretty output: %s",
         strerror(errno));
  fflush(stdout);
  saved = dup(1);
  if (saved < 0 || dup2(fileno(tmp), 1) < 0)
    fail(ERR_NO_STORE, NULL, "cannot stage --pretty output: %s",
         strerror(errno));
  print_frame(g, w, store, limit, history_depth, since);
  fflush(stdout);
  dup2(saved, 1);
  close(saved);
  n = ftell(tmp);
  if (n < 0)
    n = 0;
  buf = (char *)malloc((size_t)n + 1);
  if (!buf) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  rewind(tmp);
  if (n > 0 && fread(buf, 1, (size_t)n, tmp) != (size_t)n)
    n = 0;
  buf[n] = 0;
  fclose(tmp);
  *out_len = (u32)n;
  return buf;
}

static void emit_frame(const Hypergraph *g, const WriteReport *w,
                       const char *store, i64 limit, i32 history_depth,
                       i64 since) {
  static Json pj;
  char *buf;
  u32 len;
  if (!g_pretty) {
    print_frame(g, w, store, limit, history_depth, since);
    return;
  }
  buf = capture_frame_json(g, w, store, limit, history_depth, since, &len);
  json_parse(&pj, buf, len);
  pretty_render(&pj);
  free(buf);
}

/* The project dir a store belongs to: the store's parent (the normal flow is
 * <project>/.legend). Generated configs land beside the store — never in an
 * unrelated cwd — so a harness init against a temp store cannot write configs
 * into the repo it runs from. */
static void store_project_dir(const char *store, char *out, size_t cap) {
  char abs[4200];
  const char *slash;
  if (!realpath(store, abs))
    snprintf(abs, sizeof abs, "%s", store);
  slash = strrchr(abs, '/');
  if (!slash || slash == abs) {
    snprintf(out, cap, "/");
    return;
  }
  {
    size_t n = (size_t)(slash - abs);
    if (n >= cap)
      n = cap - 1;
    memcpy(out, abs, n);
    out[n] = 0;
  }
}

static void self_exe_path(char *exe, size_t cap) {
  ssize_t n = readlink("/proc/self/exe", exe, cap - 1);
  if (n <= 0)
    snprintf(exe, cap, "legend"); /* fall back to PATH lookup */
  else
    exe[n] = 0;
}

/* Convenience: `legend init` drops a Claude Code `.mcp.json` beside the store
 * so a session opened in this project launches `legend mcp-serve` against it —
 * one command sets a project up end to end. Never clobbers an existing file;
 * a failure here never fails init. Fills out_path with the config path;
 * returns 1 iff it wrote the file. */
static int write_mcp_config(const char *store, char *out_path, size_t out_cap) {
  char proj[4096], exe[4096], abs_store[4200];
  struct stat st;
  FILE *f;
  out_path[0] = 0;
  store_project_dir(store, proj, sizeof proj);
  snprintf(out_path, out_cap, "%s/.mcp.json", proj);
  if (stat(out_path, &st) == 0)
    return 0; /* already there: never overwrite a user's MCP config */
  self_exe_path(exe, sizeof exe);
  if (!realpath(store, abs_store))
    snprintf(abs_store, sizeof abs_store, "%s", store);
  f = fopen(out_path, "w");
  if (!f) {
    out_path[0] = 0;
    return 0;
  }
  fputs("{\n  \"mcpServers\": {\n    \"legend\": {\n      \"command\": \"", f);
  json_escape_fputs(exe, f);
  fputs("\",\n      \"args\": [\"mcp-serve\"],\n      \"env\": {\n"
        "        \"LEGEND_STATE_DIR\": \"",
        f);
  json_escape_fputs(abs_store, f);
  fputs("\"\n      }\n    }\n  }\n}\n", f);
  fclose(f);
  return 1;
}

/* `legend init` also seeds Claude Code hooks (ported from Legend v1's set) so
 * sessions in this project use memory without having to remember to:
 *   SessionStart      — inject the orientation packet as session context
 *   UserPromptSubmit  — rate-limited ambient recall: focus on the prompt text,
 *                       observe:true (a passive lookup must not train the
 *                       store), injected as context
 *   Stop              — when the session changed files, remind the model to
 *                       save durable decisions before finishing
 * v1 also nagged after EVERY file edit (PostToolUse); dropped — the Stop
 * reminder covers it without polluting each edit. Never clobbers an existing
 * settings.json; a failure never fails init. Returns 1 iff written. */
static int write_hooks_config(const char *store, char *out_path,
                              size_t out_cap) {
  char proj[4096], dir[4300], exe[4096], cmd[8704];
  struct stat st;
  FILE *f;
  out_path[0] = 0;
  store_project_dir(store, proj, sizeof proj);
  snprintf(out_path, out_cap, "%s/.claude/settings.json", proj);
  if (stat(out_path, &st) == 0)
    return 0; /* never overwrite a user's settings */
  snprintf(dir, sizeof dir, "%s/.claude", proj);
  if (mkdir(dir, 0777) != 0 && errno != EEXIST) {
    out_path[0] = 0;
    return 0;
  }
  self_exe_path(exe, sizeof exe);
  f = fopen(out_path, "w");
  if (!f) {
    out_path[0] = 0;
    return 0;
  }
  fputs("{\n  \"hooks\": {\n"
        "    \"SessionStart\": [{\"matcher\": \"*\", \"hooks\": "
        "[{\"type\": \"command\", \"command\": \"",
        f);
  /* The packet is bounded by the section caps and `limit`, so `head -c` is a
   * safety valve against a pathological store, not a routine trimmer. It was
   * doing the trimming when it was 4000: on the live trial store the cut
   * landed inside `overview`, so the model received the project scope and a
   * few active items and NEVER the decisions, constraints, open questions or
   * causal edges — the sections that are the whole point of orienting. */
  snprintf(cmd, sizeof cmd,
           "%s recall '{\"limit\":16}' --pretty 2>/dev/null | head -c 20000",
           exe);
  json_escape_fputs(cmd, f);
  fputs("\"}]}],\n"
        "    \"UserPromptSubmit\": [{\"matcher\": \"*\", \"hooks\": "
        "[{\"type\": \"command\", \"command\": \"",
        f);
  snprintf(cmd, sizeof cmd,
           "input=$(cat); now=$(date +%%s); d=${CLAUDE_PROJECT_DIR:-.}; "
           "stamp=\"$d/.legend/.last_hook_recall\"; "
           "last=$(cat \"$stamp\" 2>/dev/null || echo 0); "
           "[ $((now-last)) -lt 20 ] && exit 0; "
           /* system-injected blocks (task notifications, reminders) arrive
            * as prompts too; a leading '<' marks them — skip, not recall */
           "p0=$(printf %%s \"$input\" | "
           "sed -n 's/.*\"prompt\" *: *\"\\(.\\{1,4\\}\\).*/\\1/p'); "
           "case \"$p0\" in \"<\"*|\"\\\\u003c\"*) exit 0;; esac; "
           "p=$(printf %%s \"$input\" | "
           "sed -n 's/.*\"prompt\" *: *\"\\([^\"]*\\)\".*/\\1/p' | "
           "tr -cd 'A-Za-z0-9 _.,:()/-' | head -c 120); "
           "[ -z \"$p\" ] && exit 0; echo \"$now\" > \"$stamp\"; "
           "%s recall \"{\\\"focus\\\":[\\\"$p\\\"],\\\"observe\\\":true}\" "
           "--pretty 2>/dev/null | head -c 1500",
           exe);
  json_escape_fputs(cmd, f);
  fputs("\"}]}],\n"
        "    \"Stop\": [{\"matcher\": \"*\", \"hooks\": "
        "[{\"type\": \"command\", \"command\": \"",
        f);
  snprintf(cmd, sizeof cmd,
           "changed=$(git diff --name-only 2>/dev/null | head -5); "
           "[ -z \"$changed\" ] && exit 0; "
           "n=$(printf '%%s\\n' \"$changed\" | wc -l | tr -d ' '); "
           "printf '{\"additionalContext\":\"[Legend] %%s file(s) changed "
           "this session. Save durable decisions/facts/changes via "
           "legend_save before finishing.\"}' \"$n\"");
  json_escape_fputs(cmd, f);
  fputs("\"}]}]\n  }\n}\n", f);
  fclose(f);
  return 1;
}

/* `legend init` also drops a Codex CLI `.codex/config.toml` so a Codex session in
 * this project gets the legend_recall/legend_save MCP tools. Codex has no
 * SessionStart/UserPromptSubmit/Stop hooks (only Bash PreToolUse/PostToolUse), so
 * its recall/save nudge lives in AGENTS.md (write_agents_md) instead — the one
 * channel Codex injects on the first turn. Never clobbers; a failure never fails
 * init. Returns 1 iff it wrote the file. */
static int write_codex_config(const char *store, char *out_path, size_t out_cap) {
  char proj[4096], dir[4200], exe[4096], abs_store[4200];
  struct stat st;
  FILE *f;
  out_path[0] = 0;
  store_project_dir(store, proj, sizeof proj);
  snprintf(out_path, out_cap, "%s/.codex/config.toml", proj);
  if (stat(out_path, &st) == 0)
    return 0; /* never overwrite a user's Codex config */
  snprintf(dir, sizeof dir, "%s/.codex", proj);
  if (mkdir(dir, 0777) != 0 && errno != EEXIST) {
    out_path[0] = 0;
    return 0;
  }
  self_exe_path(exe, sizeof exe);
  if (!realpath(store, abs_store))
    snprintf(abs_store, sizeof abs_store, "%s", store);
  f = fopen(out_path, "w");
  if (!f) {
    out_path[0] = 0;
    return 0;
  }
  fputs("[mcp_servers.legend]\ncommand = \"", f);
  json_escape_fputs(exe, f); /* JSON escaping is a valid subset of TOML basic-string escaping */
  fputs("\"\nargs = [\"mcp-serve\"]\nenv = { LEGEND_STATE_DIR = \"", f);
  json_escape_fputs(abs_store, f);
  fputs("\" }\n\n"
        "[mcp_servers.legend.tools.legend_recall]\napproval_mode = \"approve\"\n\n"
        "[mcp_servers.legend.tools.legend_save]\napproval_mode = \"approve\"\n",
        f);
  fclose(f);
  return 1;
}

/* `legend init` seeds a minimal AGENTS.md so a Codex session — which has no
 * session/prompt hooks — still gets a first-turn nudge to recall at start and
 * save durable facts before finishing (Codex injects AGENTS.md guidance on the
 * first turn, the static analog of Claude's SessionStart + Stop hooks). Only
 * when absent: never clobber a project's own AGENTS.md. Returns 1 iff written. */
static int write_agents_md(const char *store, char *out_path, size_t out_cap) {
  char proj[4096];
  struct stat st;
  FILE *f;
  out_path[0] = 0;
  store_project_dir(store, proj, sizeof proj);
  snprintf(out_path, out_cap, "%s/AGENTS.md", proj);
  if (stat(out_path, &st) == 0)
    return 0; /* never overwrite a project's own AGENTS.md */
  f = fopen(out_path, "w");
  if (!f) {
    out_path[0] = 0;
    return 0;
  }
  fputs("# AGENTS.md\n\n"
        "## Legend — long-term memory\n\n"
        "This project uses **Legend**, a persistent deduplicated knowledge graph, "
        "as long-term memory across sessions, via the `legend_recall` and "
        "`legend_save` MCP tools.\n\n"
        "- At the **start** of a session, call `legend_recall` with no focus to "
        "load an orientation packet (project state, decisions, open questions).\n"
        "- **Recall before you save**, and reuse canonical names verbatim.\n"
        "- Before **finishing**, `legend_save` the durable results — decisions "
        "with reasons, next levers, negative results, and any `changes` to current "
        "values. Save what the code cannot hold.\n",
        f);
  fclose(f);
  return 1;
}

static int cmd_init(int reset) {
  char store[4200];
  const char *env = getenv("LEGEND_STATE_DIR");
  if (env && *env) {
    snprintf(store, sizeof store, "%s", env);
  } else {
    char cwd[4096];
    if (!getcwd(cwd, sizeof cwd))
      fail(ERR_NO_STORE, NULL, "cannot resolve cwd: %s", strerror(errno));
    snprintf(store, sizeof store, "%s/.legend", cwd);
  }
  if (mkdir(store, 0777) != 0 && errno != EEXIST)
    fail(ERR_NO_STORE, NULL, "cannot create store %s: %s", store,
         strerror(errno));
  acquire_lock(store);
  journal_arm(store, "init", NULL, 0);
  sweep_orphan_tmps(store);
  if (reset) {
    char snap[4300];
    snprintf(snap, sizeof snap, "%s/legend.snapshot", store);
    unlink(snap); /* ENOENT is fine; reseeding below renumbers ids (spec §8) */
  }
  if (!snapshot_load(&g_graph, store)) {
    ontology_seed(&g_graph);
    seed_ext_vocab(&g_graph);
    seed_self(&g_graph);
    snapshot_write(&g_graph, store);
  }
  /* Idempotent status report (spec §4). */
  {
    char cfg[4400], hooks[4400], codex[4400], agents[4400];
    int made = write_mcp_config(store, cfg, sizeof cfg);
    int hooks_made = write_hooks_config(store, hooks, sizeof hooks);
    int codex_made = write_codex_config(store, codex, sizeof codex);
    int agents_made = write_agents_md(store, agents, sizeof agents);
    fputs("{\"store\":\"", stdout);
    json_escape_fputs(store, stdout);
    printf("\",\"version\":%d,\"build\":\"" LEGEND_BUILD
           "\",\"elements\":%u,\"relations\":%u,\"clock\":%u",
           SNAP_VERSION, g_graph.element_count, g_graph.relation_count,
           g_graph.clock);
    fputs(",\"mcp_config\":\"", stdout);
    json_escape_fputs(cfg, stdout);
    printf("\",\"mcp_config_created\":%s", made ? "true" : "false");
    fputs(",\"hooks_config\":\"", stdout);
    json_escape_fputs(hooks, stdout);
    printf("\",\"hooks_created\":%s", hooks_made ? "true" : "false");
    fputs(",\"codex_config\":\"", stdout);
    json_escape_fputs(codex, stdout);
    printf("\",\"codex_config_created\":%s", codex_made ? "true" : "false");
    fputs(",\"agents_md\":\"", stdout);
    json_escape_fputs(agents, stdout);
    printf("\",\"agents_created\":%s}\n", agents_made ? "true" : "false");
  }
  journal_append((i64)time(NULL), 1, -1);
  return 0;
}

static void dump_stats(const Stats *s) {
  printf("\"conf\":%g,\"act\":%g,\"sal\":%g,\"stab\":%g,\"acc\":%u,\"fsc\":%u,"
         "\"sup\":%u,\"div\":%u,\"seen\":%u",
         s->confidence, s->activation, s->salience, s->stability,
         s->access_count, s->focus_success_count, s->support_count,
         s->support_diversity, s->last_seen);
}

/* Read-only full-graph dump as one JSON object: clock + every element (name,
 * aliases, summary, kind, merge tombstone, stats) + every relation (attrs,
 * status, stats). A debugging/export surface — no frame curation. */
static void dump_graph(const Hypergraph *g) {
  u32 e, r, i;
  printf("{\"clock\":%u,\"elements\":[", g->clock);
  for (e = 0; e < g->element_count; e++) {
    const Element *el = &g->elements[e];
    if (e)
      fputc(',', stdout);
    printf("{\"ref\":\"#%u\",\"name\":\"", e);
    json_escape_fputs(elem_name(g, e), stdout);
    fputc('"', stdout);
    if (el->names.count > 1) {
      fputs(",\"aliases\":[", stdout);
      for (i = 1; i < el->names.count; i++) {
        if (i > 1)
          fputc(',', stdout);
        fputc('"', stdout);
        json_escape_fputs(str_ptr(&g->strs, el->names.v[i]), stdout);
        fputc('"', stdout);
      }
      fputc(']', stdout);
    }
    if (el->summary != NONE_U32) {
      fputs(",\"summary\":\"", stdout);
      json_escape_fputs(str_ptr(&g->strs, el->summary), stdout);
      fputc('"', stdout);
    }
    if (g->elem_kind[e] != NONE_U32) {
      fputs(",\"kind\":\"", stdout);
      json_escape_fputs(elem_name(g, g->elem_kind[e]), stdout);
      fputc('"', stdout);
    }
    if (el->redirect != NONE_U32)
      printf(",\"merged_into\":\"#%u\"", el->redirect);
    fputs(",\"stats\":{", stdout);
    dump_stats(&el->stats);
    fputs("}}", stdout);
  }
  fputs("],\"relations\":[", stdout);
  for (r = 0; r < g->relation_count; r++) {
    if (r)
      fputc(',', stdout);
    printf("{\"ref\":\"rel:%u\",\"status\":\"%s\",\"attrs\":", r,
           ST_NAMES[g->relations[r].status]);
    /* dump is the archival view: every relation is already listed on its own,
     * so expanding a nested one here would print it twice and break the flat
     * shape the harness scripts parse */
    frame_put_rel_attrs_n(g, r, 0);
    fputs(",\"stats\":{", stdout);
    dump_stats(&g->relations[r].stats);
    fputs("}}", stdout);
  }
  fputs("]}\n", stdout);
}

/* ========================================================================
 * S11.5  AUDIT — `legend audit`
 *
 * A read-only scan that names the parts of a store a human should look at.
 * Every check is a deterministic graph query: no embeddings, no model, no
 * mutation, no tick. The oracle computes SUSPICION; a human decides what is
 * actually wrong. That split is deliberate — a store can hold a fact that is
 * stale on purpose (design intent the code has since drifted from), and the
 * disagreement is the signal, so nothing here may prune on its own.
 *
 * Five checks, one per papercut the trial found by hand:
 *
 *   stub        an element with no summary that is never a fact's subject —
 *               the shape left behind when a fact object silently minted a
 *               name that was never described
 *   status_fact a plain fact on a status-flavored property, which accretes
 *               and goes stale; `changes` is the verb that supersedes
 *   near_dup    two live names similar enough that dedup should have folded
 *               them and did not
 *   stale_open  a question/task with no `resolves` edge, untouched for long
 *               enough that it is probably finished or abandoned
 *   bloat       a summary that outgrew one line and should split into
 *               child elements
 *
 * Ranked most-actionable first: a stub is a defect, a bloated summary is
 * housekeeping.
 * ===================================================================== */

enum {
  AUD_PHANTOM_CLOSE = 0,
  AUD_STATUS_FACT,
  AUD_NEAR_DUP,
  AUD_PROSE_NAME,
  AUD_FLAT_DECISION,
  AUD_STALE_OPEN,
  AUD_ORPHAN,
  AUD_BLOAT,
  AUD_REASON_COUNT
};

static const char *const AUD_REASONS[AUD_REASON_COUNT] = {
    "phantom_close", "status_fact",   "near_dup", "prose_name",
    "flat_decision", "stale_open",    "orphan",   "bloat"};

/* Thresholds. Mutable so tests can drive each check from a small store. */
static u32 g_aud_bloat_chars = 400; /* a terse core + its result lands ~300; a wall starts ~400 (#66, #122) */
/* g_aud_name_chars (the prose threshold) is defined up near the error codes so the
 * save-time backstop and this prose_name check share one value. */
static u32 g_aud_stale_ticks = 50;  /* ticks of silence before an open item
                                       reads as abandoned */
static double g_aud_dup_sim = 0.72; /* name trigram Jaccard for near_dup, with
                                       differing digits vetoing the pair. Set
                                       from the trial store's own false
                                       positives: two same-topic-different-
                                       subsystem names ("regions design
                                       adversarial review" vs "loot design
                                       adversarial review") land at 0.685, so
                                       the bar sits above them */
static u32 g_aud_dup_min_name = 12; /* below this a single differing byte still
                                       leaves ~0.6 Jaccard ("elem a" vs "elem
                                       b"), so short names are incomparable by
                                       trigram and are not paired at all */

/* Suspects per reason actually printed, by default. A store that has drifted
 * for weeks has more bloated summaries than anyone will fix in one sitting,
 * and a wall of them buries the defects under the housekeeping. The full tally
 * always ships in `counts`, and whatever the cap drops is named in
 * `truncated`: the list is short, never quietly short.
 *
 * A maintenance pass needs the whole list, though — triaging 17 suspects five
 * at a time is not triage — so `limit` overrides this per call and `null`
 * lifts the cap entirely, matching how recall spells the same idea. */
static u32 g_aud_per_reason = 5;

typedef struct {
  u8 reason;
  u32 subject; /* element id, or relation id when reason is AUD_STATUS_FACT */
  u32 other;   /* near_dup partner element, else NONE_U32 */
  u32 refs;    /* live relations touching the element; triages a stub */
  u32 sev;     /* worst-first within a reason: chars, ticks silent, similarity */
} Suspect;

/* Vocabulary and structural elements are never suspects: the seeded core, the
 * extended causal vocabulary, anything used as an attribute NAME (a predicate),
 * anything used as a kind, and anything naming a provenance — whether it
 * arrived as a src/source slot or, as `source` on a save does, in a relation's
 * supporters list. All of them are summary-less and subject-less by design,
 * which is exactly the stub shape; miss one and every provenance string in the
 * store reads as a phantom. */
static void audit_mark_structural(const Hypergraph *g, u8 *structural) {
  u32 i, r, a;
  for (i = 0; i < WK_ELEMENT_COUNT && i < g->element_count; i++)
    structural[i] = 1;
  for (i = 0; i < EXT_COUNT; i++)
    if (g->wk_ext[i] != NONE_U32 && g->wk_ext[i] < g->element_count)
      structural[g->wk_ext[i]] = 1;
  for (i = 0; i < g->element_count; i++)
    if (g->elem_kind[i] != NONE_U32)
      structural[g->elem_kind[i]] = 1;
  for (r = 0; r < g->relation_count; r++) {
    const Relation *rel = &g->relations[r];
    for (a = 0; a < rel->attr_count; a++) {
      structural[rel->attrs[a].name] = 1;
      if ((rel->attrs[a].name == WK_SOURCE || rel->attrs[a].name == WK_SRC) &&
          rel->attrs[a].value.tag == TERM_ELEM)
        structural[rel->attrs[a].value.id] = 1;
    }
    for (a = 0; a < rel->supporters.count; a++)
      structural[rel->supporters.v[a]] = 1;
  }
}

/* A property whose value moves over time. Trial doc §11: these belong in
 * `changes`, which supersedes and keeps history; written as a plain fact they
 * accrete and the old value stays live next to the new one. current_* names
 * are the cache `changes` already wrote, so they are the correct shape. */
static int audit_name_is_status_flavored(const Hypergraph *g, u32 name_elem) {
  static const char *const FLAVORS[] = {"status",  "state",   "progress",
                                        "phase",   "standing", "stage",
                                        "version", "result"};
  const char *nb = elem_name(g, name_elem);
  u32 nl = elem_name_l(g, name_elem), i;
  if (nl > 8 && memcmp(nb, "current_", 8) == 0)
    return 0;
  for (i = 0; i < sizeof FLAVORS / sizeof FLAVORS[0]; i++) {
    u32 fl = (u32)strlen(FLAVORS[i]);
    if (nl == fl && memcmp(nb, FLAVORS[i], fl) == 0)
      return 1;
  }
  return 0;
}

/* Live relations touching `e`, and whether any of them is a plain fact with
 * `e` as its subject. An element that is never a subject was named by someone
 * else and never described in its own right. */
static u32 audit_elem_refs(const Hypergraph *g, u32 e, int *is_subject) {
  u32 link = g->rels_by_elem[e], refs = 0;
  *is_subject = 0;
  while (link != NONE_U32) {
    u32 rid = g->rel_links[link].rel;
    const Relation *r = &g->relations[rid];
    u32 a;
    link = g->rel_links[link].next;
    if (r->status >= ST_SUPERSEDED)
      continue;
    refs++;
    for (a = 0; a < r->attr_count; a++)
      if (r->attrs[a].name == WK_SUBJECT && r->attrs[a].value.tag == TERM_ELEM &&
          r->attrs[a].value.id == e && r->attr_count == 2 &&
          r->attrs[0].name != WK_INSTANCE_OF && r->attrs[1].name != WK_INSTANCE_OF)
        *is_subject = 1;
  }
  return refs;
}

/* Whether any live relation closes `e` — a {subject: X, resolves: e} fact.
 * Rewriting a question's summary does not close it; only this edge does. */
static int audit_is_resolved(const Hypergraph *g, u32 e) {
  u32 link = g->rels_by_elem[e];
  while (link != NONE_U32) {
    u32 rid = g->rel_links[link].rel;
    const Relation *r = &g->relations[rid];
    u32 a;
    link = g->rel_links[link].next;
    if (r->status >= ST_SUPERSEDED)
      continue;
    for (a = 0; a < r->attr_count; a++)
      if (r->attrs[a].name == WK_RESOLVES &&
          r->attrs[a].value.tag == TERM_ELEM && r->attrs[a].value.id == e)
        return 1;
  }
  return 0;
}

/* The digits of a canonical name, in order ("trial round 2" -> "2"). Names in
 * a real store distinguish siblings by number or date far more often than by
 * wording — round 1 vs round 2, session 2 vs session 3, testimony 5 vs 6 —
 * and trigram similarity barely registers the one byte that carries all the
 * meaning: measured on this store's own names, those DISTINCT pairs score
 * 0.83-0.89 while a genuine typo duplicate scores 0.80, so no threshold can
 * separate them. Differing digits therefore veto a near_dup outright. Same
 * reading of digits as rel_exception_protected: in this store they are
 * identity, not decoration. */
static u32 audit_name_digits(const Hypergraph *g, u32 e, char *out, u32 cap) {
  const char *b = elem_name(g, e);
  u32 n = elem_name_l(g, e), i, w = 0;
  for (i = 0; i < n && w < cap; i++)
    if (b[i] >= '0' && b[i] <= '9')
      out[w++] = b[i];
  return w;
}

/* Whether these two similar names are similar ON PURPOSE — a pair the graph
 * itself says belongs together, which folding would damage. Two shapes:
 *
 *   from/to of one change event. `changes` records supersession as
 *   {from: <old>, to: <new>}, so both values survive as elements, and a
 *   property whose value was EDITED rather than replaced leaves two
 *   near-identical names by design ("...Rare/Epic/Mythic" ->
 *   "...Rare/Fabled/Mythic"). Merging them destroys the history `changes`
 *   exists to keep.
 *
 *   parent and child. Splitting an overgrown summary into a short core plus
 *   children is the documented remedy for `bloat`, and a child is naturally
 *   named after its parent ("color-signature summons roster" under
 *   "color-signature summons"). Without this, taking the advice the tool gives
 *   you manufactures suspects in another of its own checks.
 *
 * Only reached for pairs that already cleared the similarity bar, so the scan
 * costs nothing. */
static int audit_pair_is_intentional(const Hypergraph *g, u32 a, u32 b) {
  u32 r, i;
  for (r = 0; r < g->relation_count; r++) {
    const Relation *rel = &g->relations[r];
    u32 from = NONE_U32, to = NONE_U32, subj = NONE_U32, part = NONE_U32;
    for (i = 0; i < rel->attr_count; i++) {
      if (rel->attrs[i].value.tag != TERM_ELEM)
        continue;
      if (rel->attrs[i].name == WK_FROM)
        from = rel->attrs[i].value.id;
      else if (rel->attrs[i].name == WK_TO)
        to = rel->attrs[i].value.id;
      else if (rel->attrs[i].name == WK_SUBJECT)
        subj = rel->attrs[i].value.id;
      else if (rel->attrs[i].name == WK_PART_OF)
        part = rel->attrs[i].value.id;
    }
    if ((from == a && to == b) || (from == b && to == a))
      return 1;
    if (rel->status < ST_SUPERSEDED &&
        ((subj == a && part == b) || (subj == b && part == a)))
      return 1;
  }
  return 0;
}

static int audit_kind_is_open(const Hypergraph *g, u32 e) {
  u32 k = g->elem_kind[e];
  return k != NONE_U32 && (k == WK_KIND_QUESTION || k == WK_KIND_TASK);
}

static int suspect_cmp(const void *pa, const void *pb) {
  const Suspect *a = (const Suspect *)pa, *b = (const Suspect *)pb;
  if (a->reason != b->reason)
    return a->reason < b->reason ? -1 : 1;
  if (a->sev != b->sev)
    return a->sev > b->sev ? -1 : 1;
  return a->subject < b->subject ? -1 : a->subject > b->subject ? 1 : 0;
}

static void audit_put_suspect(const Hypergraph *g, const Suspect *s, int first) {
  if (!first)
    fputc(',', stdout);
  printf("{\"reason\":\"%s\",", AUD_REASONS[s->reason]);
  if (s->reason == AUD_STATUS_FACT) {
    printf("\"ref\":\"rel:%u\",\"attrs\":", s->subject);
    frame_put_rel_attrs(g, s->subject);
  } else {
    const Element *el = &g->elements[s->subject];
    printf("\"ref\":\"#%u\",\"name\":\"", s->subject);
    json_escape_fputs(elem_name(g, s->subject), stdout);
    fputc('"', stdout);
    if (g->elem_kind[s->subject] != NONE_U32) {
      fputs(",\"kind\":\"", stdout);
      json_escape_fputs(elem_name(g, g->elem_kind[s->subject]), stdout);
      fputc('"', stdout);
    }
    if (s->reason == AUD_ORPHAN)
      printf(",\"refs\":%u", s->refs);
    if (s->reason == AUD_PROSE_NAME)
      printf(",\"name_chars\":%u", s->sev);
    if (s->reason == AUD_PHANTOM_CLOSE) {
      printf(",\"via\":\"rel:%u\",\"fact\":", s->other);
      frame_put_rel_attrs(g, s->other);
    }
    if (s->reason == AUD_NEAR_DUP) {
      printf(",\"other\":\"#%u\",\"similarity\":%.3f,\"other_name\":\"",
             s->other, (double)s->sev / 1000.0);
      json_escape_fputs(elem_name(g, s->other), stdout);
      fputc('"', stdout);
    }
    if (s->reason == AUD_STALE_OPEN)
      printf(",\"last_seen\":%u,\"silent_for\":%u", el->stats.last_seen,
             g->clock - el->stats.last_seen);
    if (s->reason == AUD_BLOAT)
      printf(",\"summary_chars\":%u", str_len(&g->strs, el->summary));
  }
  fputc('}', stdout);
}

/* Run every check and hand back the suspects, ranked. Nothing here writes, so
 * a scan can run against a live store mid-session.
 *
 * `with_near_dup` exists for one caller: the orientation packet, which wants a
 * tally cheaply. The pair sweep is O(n^2) and measures at 58ms of a 59ms scan
 * on an 815-element store, against the 4.6ms the whole orientation recall
 * costs — folding it into session start would make boot an order of magnitude
 * slower and get worse as the store grows. Skipping it leaves the scan at
 * ~4ms, and near_dup is also the least precise check, so the ambient tally
 * gives up nothing it could act on. A deliberate `legend audit` still runs
 * everything. */
static void audit_scan(const Hypergraph *g, Suspect **out_sus, u32 *out_n,
                       u32 *counts, int with_near_dup) {
  u8 *structural = (u8 *)calloc(g->element_count ? g->element_count : 1, 1);
  u8 *dec_content = (u8 *)calloc(g->element_count ? g->element_count : 1, 1);
  u8 *is_option = (u8 *)calloc(g->element_count ? g->element_count : 1, 1);
  Suspect *sus = NULL;
  u32 cap = 0, n = 0, e, r, i;
  U32Vec ta, tb;
  if (!structural || !dec_content || !is_option) {
    fprintf(stderr, "legend: out of memory\n");
    exit(1);
  }
  memset(&ta, 0, sizeof ta);
  memset(&tb, 0, sizeof tb);
  for (i = 0; i < AUD_REASON_COUNT; i++)
    counts[i] = 0;
  audit_mark_structural(g, structural);

  /* Which elements carry a decision's defining content (dec_content: at least
   * one live chose/rejected/about/resolves relation — a decision with none
   * recorded its choice only in its name + summary, the "Bolt as the default"
   * shape), and which are the option/target VALUES those slots point at
   * (is_option). An option that a hub broke out is not itself a flat decision
   * even if it happens to carry kind:decision. */
  for (r = 0; r < g->relation_count; r++) {
    const Relation *rel = &g->relations[r];
    u32 a, subj = NONE_U32;
    int has = 0;
    if (rel->status >= ST_SUPERSEDED)
      continue;
    for (a = 0; a < rel->attr_count; a++) {
      if (rel->attrs[a].name == WK_SUBJECT &&
          rel->attrs[a].value.tag == TERM_ELEM)
        subj = rel->attrs[a].value.id;
      else if (rel->attrs[a].name == WK_CHOSE ||
               rel->attrs[a].name == WK_REJECTED ||
               rel->attrs[a].name == WK_ABOUT ||
               rel->attrs[a].name == WK_RESOLVES) {
        has = 1;
        if (rel->attrs[a].value.tag == TERM_ELEM &&
            rel->attrs[a].value.id < g->element_count)
          is_option[rel->attrs[a].value.id] = 1;
      }
    }
    if (has && subj < g->element_count)
      dec_content[subj] = 1;
  }

#define AUD_PUSH(rsn, subj, oth, rf, sv)                                       \
  do {                                                                         \
    sus = (Suspect *)xgrow(sus, n + 1, &cap, sizeof *sus);                      \
    sus[n].reason = (u8)(rsn);                                                  \
    sus[n].subject = (subj);                                                    \
    sus[n].other = (oth);                                                       \
    sus[n].refs = (rf);                                                         \
    sus[n].sev = (sv);                                                          \
    n++;                                                                        \
    counts[rsn]++;                                                              \
  } while (0)

  for (e = 0; e < g->element_count; e++) {
    const Element *el = &g->elements[e];
    int is_subject;
    u32 refs;
    if (structural[e] || el->redirect != NONE_U32)
      continue;
    refs = audit_elem_refs(g, e, &is_subject);
    /* A summary-less element referenced by a live fact is NOT reportable on
     * that shape alone: "sdl" and "game engine" are perfectly good fact
     * objects that will never carry a summary, and they outnumber real
     * defects 119 to 5 in the trial store. What separates a mistake from a
     * shorthand is not the missing summary — it is prose where a name
     * belongs, or a `resolves` that closed nothing. Both are checked
     * directly, so nothing keys off the summary being absent.
     *
     * The one exception is an element NOTHING references, which no fact can
     * be reading: a provenance string whose facts all deduped, or what a
     * retraction left behind. Inert, so it ranks near the bottom. */
    if (el->summary == NONE_U32 && !is_subject && refs == 0)
      AUD_PUSH(AUD_ORPHAN, e, NONE_U32, refs, 0);
    if (elem_name_l(g, e) > g_aud_name_chars)
      AUD_PUSH(AUD_PROSE_NAME, e, NONE_U32, refs, elem_name_l(g, e));
    if (g->elem_kind[e] == WK_KIND_DECISION && !dec_content[e] && !is_option[e])
      AUD_PUSH(AUD_FLAT_DECISION, e, NONE_U32, refs, 0);
    if (el->summary != NONE_U32 && str_len(&g->strs, el->summary) > g_aud_bloat_chars)
      AUD_PUSH(AUD_BLOAT, e, NONE_U32, refs, str_len(&g->strs, el->summary));
    if (audit_kind_is_open(g, e) && !audit_is_resolved(g, e) &&
        g->clock > g_aud_stale_ticks &&
        el->stats.last_seen < g->clock - g_aud_stale_ticks)
      AUD_PUSH(AUD_STALE_OPEN, e, NONE_U32, refs,
               g->clock - el->stats.last_seen);
  }

  for (r = 0; r < g->relation_count; r++) {
    const Relation *rel = &g->relations[r];
    u32 a, subj = NONE_U32, prop = NONE_U32;
    if (rel->status >= ST_SUPERSEDED)
      continue;
    /* A `resolves` whose target was minted by this very fact closed nothing —
     * it invented a leaf shaped like an answer. An open item always carries a
     * kind (question or task); a target with no kind at all was never on the
     * books, so the question it claims to close is still open and now has a
     * decoy sitting next to it. The trial store holds exactly one, which is
     * the shape of a check worth running. */
    for (a = 0; a < rel->attr_count; a++)
      if (rel->attrs[a].name == WK_RESOLVES &&
          rel->attrs[a].value.tag == TERM_ELEM &&
          g->elem_kind[rel->attrs[a].value.id] == NONE_U32)
        AUD_PUSH(AUD_PHANTOM_CLOSE, rel->attrs[a].value.id, r, 0, 0);
    if (rel->attr_count != 2)
      continue;
    for (a = 0; a < 2; a++) {
      if (rel->attrs[a].name == WK_SUBJECT && rel->attrs[a].value.tag == TERM_ELEM)
        subj = rel->attrs[a].value.id;
      else
        prop = rel->attrs[a].name;
    }
    if (subj == NONE_U32 || prop == NONE_U32 || structural[subj])
      continue;
    if (audit_name_is_status_flavored(g, prop))
      AUD_PUSH(AUD_STATUS_FACT, r, NONE_U32, 0, 0);
  }

  /* Near-duplicate names, O(n^2) over live non-structural elements. A store
   * is thousands of elements at most and audit runs on demand, so the pair
   * sweep stays well under the cost of the snapshot load that preceded it. */
  for (e = 0; with_near_dup && e < g->element_count; e++) {
    char da[32], db[32];
    u32 dan, dbn;
    if (structural[e] || g->elements[e].redirect != NONE_U32 ||
        elem_name_l(g, e) < g_aud_dup_min_name)
      continue;
    tier2_trigrams(elem_name(g, e), elem_name_l(g, e), &ta);
    dan = audit_name_digits(g, e, da, sizeof da);
    for (r = e + 1; r < g->element_count; r++) {
      double sim;
      if (structural[r] || g->elements[r].redirect != NONE_U32 ||
          elem_name_l(g, r) < g_aud_dup_min_name)
        continue;
      dbn = audit_name_digits(g, r, db, sizeof db);
      if (dan != dbn || (dan && memcmp(da, db, dan) != 0))
        continue;
      tier2_trigrams(elem_name(g, r), elem_name_l(g, r), &tb);
      sim = trigram_jaccard(ta.v, ta.count, tb.v, tb.count);
      if (sim >= g_aud_dup_sim && !audit_pair_is_intentional(g, e, r))
        AUD_PUSH(AUD_NEAR_DUP, r, e, 0, (u32)(sim * 1000.0));
    }
  }
#undef AUD_PUSH

  if (n > 1)
    qsort(sus, n, sizeof *sus, suspect_cmp);
  free(structural);
  free(dec_content);
  free(is_option);
  free(ta.v);
  free(tb.v);
  *out_sus = sus;
  *out_n = n;
}

/* The summary-length distribution over the elements the bloat check ranges on.
 * The bloat COUNT tracks store size rather than health (#122, #153): splitting a
 * wall into a short core plus children moves it the wrong way, because the detail
 * still has to live somewhere and every child clears the threshold too. What
 * moved on that same split was max (4047 -> 1405), so max/p90/total is the metric
 * that answers "are summaries becoming walls". */
static void audit_put_prose(const Hypergraph *g) {
  u8 *structural = (u8 *)calloc(g->element_count ? g->element_count : 1, 1);
  u32 *len = NULL, n = 0, cap = 0, e;
  u64 total = 0;
  audit_mark_structural(g, structural);
  for (e = 0; e < g->element_count; e++) {
    const Element *el = &g->elements[e];
    if (structural[e] || el->redirect != NONE_U32 || el->summary == NONE_U32)
      continue;
    len = (u32 *)xgrow(len, n + 1, &cap, sizeof *len);
    len[n] = str_len(&g->strs, el->summary);
    total += len[n++];
  }
  if (n)
    qsort(len, n, sizeof *len, u32_cmp);
  printf(",\"prose\":{\"summaries\":%u,\"chars\":%lu,\"max\":%u,\"p90\":%u,"
         "\"mean\":%u,\"over\":%u}",
         n, (unsigned long)total, n ? len[n - 1] : 0,
         n ? len[(n * 9) / 10 < n ? (n * 9) / 10 : n - 1] : 0,
         n ? (u32)(total / n) : 0, g_aud_bloat_chars);
  free(len);
  free(structural);
}

/* Rank, cap, print. `per_reason` < 0 prints every suspect. */
static void audit_graph(const Hypergraph *g, i64 per_reason) {
  Suspect *sus;
  u32 n, i;
  u32 counts[AUD_REASON_COUNT];
  audit_scan(g, &sus, &n, counts, 1);
  printf("{\"clock\":%u,\"elements\":%u,\"suspects\":[", g->clock,
         g->element_count);
  {
    u32 shown[AUD_REASON_COUNT], put = 0;
    for (i = 0; i < AUD_REASON_COUNT; i++)
      shown[i] = 0;
    for (i = 0; i < n; i++) {
      if (per_reason >= 0 && shown[sus[i].reason] >= (u32)per_reason)
        continue;
      shown[sus[i].reason]++;
      audit_put_suspect(g, &sus[i], put++ == 0);
    }
    fputs("],\"counts\":{", stdout);
    for (i = 0; i < AUD_REASON_COUNT; i++)
      printf("%s\"%s\":%u", i ? "," : "", AUD_REASONS[i], counts[i]);
    fputs("},\"truncated\":{", stdout);
    {
      u32 first = 1;
      for (i = 0; i < AUD_REASON_COUNT; i++)
        if (counts[i] > shown[i]) {
          printf("%s\"%s\":%u", first ? "" : ",", AUD_REASONS[i],
                 counts[i] - shown[i]);
          first = 0;
        }
    }
    printf("},\"shown\":%u,\"total\":%u", put, n);
    audit_put_prose(g);
    fputs("}\n", stdout);
  }
  free(sus);
}

/* `{"limit": N}` caps suspects per reason; `{"limit": null}` lifts the cap;
 * an absent payload keeps the default. Spelled exactly as recall spells it. */
static i64 read_audit_limit(const Rd *r) {
  u32 n, ti, k;
  if (r->t[0].type != J_OBJ)
    fail(ERR_PARSE, NULL, "payload must be a JSON object");
  n = r->t[0].size;
  ti = 1;
  for (k = 0; k < n; k++) {
    u32 key = ti, val = ti + 1;
    if (rd_str_eq(r, key, "limit")) {
      if (r->t[val].type == J_NULL)
        return -1;
      {
        i64 v = rd_integer(r, val, "limit");
        if (v < 1)
          fail(ERR_PARSE, "limit", "expected a positive integer or null");
        return v;
      }
    }
    fail(ERR_PARSE, NULL, "audit takes only `limit`");
    ti = tok_skip(r->t, val);
  }
  return (i64)g_aud_per_reason;
}

/* The session-start tally: the same checks, minus the O(n^2) pair sweep,
 * emitted into the orientation overview. Placement is load-bearing — the
 * SessionStart hook truncates the packet with `head -c 4000` and that cut now
 * lands inside overview.active, so a counter anywhere further down would never
 * reach the model. Emitted only when something is flagged: a clean store says
 * nothing, and maintenance stays the user's move rather than a standing nag. */
static void frame_put_audit_tally(const Hypergraph *g) {
  Suspect *sus;
  u32 n, i, put = 0, shown = 0;
  u32 counts[AUD_REASON_COUNT];
  audit_scan(g, &sus, &n, counts, 0);
  free(sus);
  /* bloat is excluded from the standing tally: its count climbs with store
   * size and cannot be acted on mid-session, so it belongs in `legend audit`,
   * not the session-start signal (#122, confirmed by trial round 4 — count
   * rose with element count while the distribution held flat). near_dup is
   * already 0 here (the scan skipped the O(n^2) sweep). */
  counts[AUD_BLOAT] = 0;
  for (i = 0; i < AUD_REASON_COUNT; i++)
    shown += counts[i];
  if (shown == 0)
    return;
  fputs(",\"audit\":{", stdout);
  for (i = 0; i < AUD_REASON_COUNT; i++) {
    if (counts[i] == 0)
      continue;
    printf("%s\"%s\":%u", put++ ? "," : "", AUD_REASONS[i], counts[i]);
  }
  fputc('}', stdout);
}

/* Parse argv, dispatch the verb, and run the one invocation. init creates or
 * reports the store; save/recall discover an existing store, lock it, load,
 * tick, save (unless observe), and print. Every error along the way funnels
 * through fail() to the single JSON error exit. */
/* ========================================================================
 * S12  MCP SERVER — `legend mcp-serve`
 *
 * A long-lived process speaking MCP (JSON-RPC 2.0 over newline-delimited
 * stdio). One process keeps the embedding model (embed.c global) warm across
 * every tools/call instead of paying ~55ms of reload per CLI spawn. Each call
 * reloads the snapshot fresh (cheap) so a rejected write can never poison a
 * later one. Tools reuse the exact CLI path — read_submission / read_recall ->
 * tick_save / tick_recall -> emit_frame — over the tool `arguments` object
 * (an Rd cursor points straight at that token subtree). The only new surface
 * is the protocol envelope and capturing the frame emit_frame() prints to
 * stdout (which is the protocol channel here) into the JSON-RPC result. A
 * fail() mid-call is trapped and returned as an isError tool result rather
 * than killing the server.
 * ===================================================================== */

static const char MCP_INSTRUCTIONS[] =
    "Legend is your long-term memory across sessions: a deduplicated, revisable "
    "knowledge graph, not a chat log. ELEMENTS are durable named things "
    "(project, function, module, system, mechanic, parameter, constraint, "
    "decision, spell, enemy, character, place, question); each has a canonical "
    "name, a kind, optional aliases, and a one-line summary. FACTS are {s,p,o} "
    "triples on elements. A NAME is a short NOUN -- the subject of one "
    "intentional thing (`trap spells`, `Epic`, `the spell bar`): at most 5 "
    "words, most just one or two, and NEVER a claim, sentence, status, or log -- "
    "a name shaped `X as the Y`, `X over Y`, `no X`, `X dropped/cut`, or `X is Y` "
    "is a claim wearing a noun costume; split it into the entities plus a fact. "
    "A CLAIM -- anything asserting something between entities -- is a FACT "
    "between them, never a name: if your name would assert something, name the "
    "entities (the nouns) and write the claim as a fact; detail and reasoning "
    "go in the summary or in facts, never the name. Discipline: (1) recall "
    "before you save, to find the "
    "canonical name of anything that already exists and reuse it verbatim -- "
    "never mint a second element for the same thing. Reuse an existing "
    "predicate; prefer short verbs. (2) To change a current "
    "value use `changes` {target, property, to}, not a new fact -- changes "
    "supersede and keep history. Status-like values go through changes, not "
    "facts; the `changes` target must ALREADY EXIST or it silently mints a "
    "phantom while the real value stays stale, and `to` is a SHORT canonical "
    "value that itself becomes an element -- never a prose note (that belongs "
    "in the target's summary). (3) To correct something now false use "
    "`retract`. (4) To fold a duplicate you made use `merge` {from, into}. "
    "(5) To rename, set `rename_to` on the element. (6) To close an open "
    "question or task, save the fact {s: <what resolved it>, p: resolves, "
    "o: <the question/task>} -- rewriting its summary does NOT close it, and "
    "the o must ALREADY EXIST (a fact silently mints unknown refs: if your "
    "resolves target shows up in the frame's writes.minted_elements, you "
    "created a phantom -- there was no open item to close; retract it). "
    "(7) To update an element's summary, resubmit the element with the new "
    "summary (latest write wins); to CORRECT its kind, resubmit with the new "
    "`kind` (it supersedes the old, latest wins). `changes` is for domain property VALUES "
    "(a fact object -- and every attr value -- becomes an element named by it, "
    "so never pass prose as a fact object or an attr value; prose belongs in "
    "summaries). When a summary outgrows a line, "
    "split the detail into child elements and keep a short core (under "
    "~400 chars) -- an "
    "everything-dump summary buries the signal. (8) Prefer few precise "
    "elements and durable facts; over-extraction buries the signal. The "
    "highest-value saves are what the code cannot hold: next levers, "
    "negative results, decisions with reasons. A choice that settles a design "
    "question is a decision even when the request looked cosmetic. A decision is "
    "a HUB, not a headline: name it for the topic it settles (a noun) and record "
    "the choice as STRUCTURE -- chose and rejected point at the option ENTITIES "
    "(so they become real, linkable nodes -- each takes its OWN kind, a language "
    "or library or mechanic, never kind:decision), reason holds the why, "
    "about/resolves link what it decides. Never bury the choice in the name or "
    "summary: "
    "`Bolt as the default` should be a decision named `default fumble shape` with "
    "chose=Bolt, rejected=Melee, reason=`Bolt is the neutral option`, not one "
    "opaque node. (9) A saved "
    "measurement "
    "without its method is half lost -- record how to reproduce it (a src "
    "pointer or a harness/pointer element). (10) For cause and effect, use the "
    "predicates caused / enables / prevents for a real causal claim and "
    "correlated_with for mere co-occurrence -- keep them distinct, never call a "
    "correlation a cause. Tag a claim's modality with a fact `modal` array: "
    "intervened (an agent acted, vs the default of an observed fact), non_actual "
    "(counterfactual or merely desired, not actual state), negated, uncertain, "
    "general (habitual/generic). Recall tags each fact with its modality "
    "wherever it surfaces -- a `negated` fact is asserted to be FALSE, so read "
    "the modal, don't take the bare claim -- and gathers causal edges into a "
    "`causal` section with their rung. (11) This store is YOUR memory and you "
    "are IN it: the seeded element `me` is you, the agent reading and writing "
    "here. Write yourself in wherever you are a DISTINGUISHING participant -- "
    "a directive that binds you, a correction you were given, an error you "
    "repeated, a standing commitment you now hold -- so that a later session "
    "recalls what it was ASKED, not just what is true. Resolve person at write "
    "time: the user's `you` is `me`, the user's `I` is the user by name, and no "
    "pronoun other than `me` ever enters a name or a fact slot (inside a quoted "
    "source string it is opaque text and stays as written). Do NOT anchor "
    "authorship on `me` -- every save here is yours, so `source` stays the "
    "material you drew on, never the fact that you wrote it. (12) A statement "
    "can be the CONTENT of another statement, and that is how a request, a "
    "report, or a quote is written. `Nick asked me to stop using em dashes` is "
    "ONE fact {s: Nick, p: asked, o: me} carrying the asked-for statement in a "
    "`content` slot -- NOT two loose facts, which lose both the pivot (the same "
    "`me` in two roles) and the containment ({Nick asked me} stops saying WHAT, "
    "and {me removes em dash} asserts as standing truth what was only "
    "requested). Mark the inner statement `modal:[\"non_actual\"]` when it is "
    "wanted rather than already true. A `content` slot takes a rel: id, which "
    "means TWO saves: write the inner statement, read its id from the frame's "
    "writes.minted_relations, then write the outer one referencing it. Recall "
    "expands a nested statement inline with its modal, and reaches a container "
    "from any term inside it, so the reader gets the whole. Recall "
    "with no focus returns an orientation packet for session start.";

static const char MCP_TOOLS_JSON[] =
    "[{\"name\":\"legend_recall\",\"description\":\"Read memory. With `focus` "
    "(entity names or topics) returns those elements, their current facts, and "
    "related context. With NO focus returns a session-start orientation packet "
    "(project, state, decisions, constraints, open questions). Call this BEFORE "
    "saving to find the canonical name of anything that already exists.\","
    "\"inputSchema\":{\"type\":\"object\",\"properties\":{"
    "\"focus\":{\"type\":\"array\",\"items\":{\"type\":\"string\"},"
    "\"description\":\"entity names or topics; omit for an orientation packet\"},"
    "\"query\":{\"type\":\"string\",\"description\":\"optional: what you want to "
    "know; ranks the focus's facts by relevance to it. Give the discriminating "
    "intent (e.g. 'date of birth'), NOT the whole question or the entity name "
    "again. Omit for a general recall (recency order).\"},"
    "\"limit\":{\"type\":\"integer\",\"description\":\"max related items (default 40)\"},"
    "\"history_depth\":{\"type\":\"integer\",\"description\":\"past values per fact (default 2)\"},"
    "\"observe\":{\"type\":\"boolean\",\"description\":\"read-only; do not record the access\"}}}},"
    "{\"name\":\"legend_save\",\"description\":\"Write to memory. Recall first, "
    "then reuse canonical names verbatim. Reuse an existing predicate; prefer "
    "short verbs. To CHANGE a current value use "
    "`changes` (not a new fact) -- it supersedes and keeps history. Status-like "
    "values go through changes, not facts. To correct "
    "something now false use `retract`. To fold a duplicate you made use "
    "`merge`. To rename an element set `rename_to`. To CLOSE an open "
    "question/task save a fact {s: <resolver>, p: resolves, o: <the task>} -- "
    "editing its summary does not close it, and the o must already exist "
    "(check writes.minted_elements in the frame: your target there = a "
    "phantom, retract it). To UPDATE a summary resubmit the "
    "element (latest write wins) -- `changes` is for domain property values, "
    "and fact objects AND attr values become element names, so keep them short "
    "(prose goes in summaries); when a summary outgrows a line, split the detail into "
    "child elements and keep a short core (under ~400 chars). Best saves: what code cannot hold "
    "(next levers, negative "
    "results, reasons); a choice that settles a design question is a decision "
    "even when the request looked cosmetic; measurements include how to "
    "reproduce them. For cause and effect use predicates caused/enables/prevents "
    "(a real cause) vs correlated_with (co-occurrence only), never conflating "
    "the two; tag a fact's modality with `modal` "
    "(intervened|non_actual|negated|uncertain|general). Prefer "
    "few precise "
    "elements; over-extraction buries the signal. At least one write list is "
    "required.\",\"inputSchema\":{\"type\":\"object\",\"properties\":{"
    "\"source\":{\"type\":\"string\",\"description\":\"provenance for facts minted this call\"},"
    "\"elements\":{\"type\":\"array\",\"items\":{\"type\":\"object\",\"properties\":{"
    "\"name\":{\"type\":\"string\"},"
    "\"kind\":{\"type\":\"string\",\"description\":\"project|function|module|system|mechanic|"
    "parameter|constraint|decision|spell|enemy|character|place|question|person|file|event|task|pointer\"},"
    "\"summary\":{\"type\":\"string\",\"description\":\"one line\"},"
    "\"aliases\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}},"
    "\"attrs\":{\"type\":\"object\",\"description\":\"structured properties (name -> value(s)); each value becomes an element, so use canonical names, not prose\"},"
    "\"rename_to\":{\"type\":\"string\",\"description\":\"new canonical name for this element\"},"
    "\"src\":{\"type\":\"string\",\"description\":\"source pointer (file/url/id)\"}},"
    "\"required\":[\"name\"]}},"
    "\"facts\":{\"type\":\"array\",\"items\":{\"type\":\"object\",\"properties\":{"
    "\"s\":{\"type\":\"string\"},\"p\":{\"type\":\"string\"},\"o\":{\"type\":\"string\"},"
    "\"status\":{\"type\":\"string\",\"enum\":[\"asserted\",\"defeasible\"],"
    "\"description\":\"defeasible = tentative/uncertain\"},"
    "\"modal\":{\"type\":\"array\",\"items\":{\"type\":\"string\","
    "\"enum\":[\"intervened\",\"non_actual\",\"negated\",\"uncertain\",\"general\"]},"
    "\"description\":\"claim modality, reified as meta-relations\"}},"
    "\"required\":[\"s\",\"p\",\"o\"]}},"
    "\"changes\":{\"type\":\"array\",\"description\":\"supersede a current value (keeps history)\","
    "\"items\":{\"type\":\"object\",\"properties\":{"
    "\"target\":{\"type\":\"string\",\"description\":\"must already exist, else changes mints a phantom\"},\"property\":{\"type\":\"string\"},"
    "\"from\":{\"type\":\"string\",\"description\":\"prior value if known\"},"
    "\"to\":{\"type\":\"string\",\"description\":\"new value -- short canonical name; becomes an element, never prose\"}},\"required\":[\"target\",\"property\",\"to\"]}},"
    "\"retract\":{\"type\":\"array\",\"description\":\"mark a fact no longer true\","
    "\"items\":{\"type\":\"object\",\"properties\":{"
    "\"s\":{\"type\":\"string\"},\"p\":{\"type\":\"string\"},\"o\":{\"type\":\"string\"}},"
    "\"required\":[\"s\",\"p\",\"o\"]}},"
    "\"merge\":{\"type\":\"array\",\"description\":\"fold a duplicate element into the canonical one\","
    "\"items\":{\"type\":\"object\",\"properties\":{"
    "\"from\":{\"type\":\"string\"},\"into\":{\"type\":\"string\"}},"
    "\"required\":[\"from\",\"into\"]}}}}},"
    "{\"name\":\"legend_audit\",\"description\":\"Scan the store for entries a "
    "HUMAN should adjudicate, ranked most-actionable first. Read-only: it "
    "changes nothing and is safe to run any time. Reasons: `phantom_close` (a "
    "resolves that closed nothing and left a decoy), `status_fact` (a "
    "changing value written as a plain fact instead of `changes`), "
    "`near_dup` (two names dedup should probably have folded), `prose_name` "
    "(a sentence where a canonical name belongs), `flat_decision` (a decision "
    "whose choice lives only in its name/summary -- no chose/rejected/about/"
    "resolves, so the entities it decides over were never broken out), "
    "`stale_open` (an open item "
    "long untouched), `orphan` (nothing references it), `bloat` (a summary "
    "that should split into children). `counts` is the full tally even when "
    "the printed list is capped per reason; `truncated` names what was "
    "dropped. NOT a to-do list and NOT permission to clean: a suspect is a "
    "question for the user, and a fact that disagrees with the code may be "
    "recording intent worth keeping. Propose, never repair unasked.\","
    "\"inputSchema\":{\"type\":\"object\",\"properties\":{"
    "\"limit\":{\"type\":[\"integer\",\"null\"],\"description\":\"suspects "
    "shown per reason (default 5); null for all of them -- pass null when "
    "actually working through a reason group, since triaging 17 suspects 5 "
    "at a time is not triage\"}}}}]";

/* `prompts/get maintain` — the human-in-the-loop gardening session. Served
 * over MCP prompts (clients surface it as a slash command) beside `onboard`,
 * so the flow travels with the binary. The division of labour is the whole
 * point: `legend_audit` computes suspicion deterministically, the model only
 * presents and executes, and the USER decides. Nothing here may repair on its
 * own — a store can hold a fact that is stale on purpose, and that
 * disagreement with the code has diagnostic value an eager tidier destroys. */
static const char MCP_MAINTAIN_PROMPT[] =
    "Run a maintenance pass over the Legend store WITH the user. You are "
    "presenting evidence and carrying out their decisions; you are not "
    "cleaning up. Never repair anything they did not just approve.\n\n"
    "Step 1 -- SCAN. Call `legend_audit`. If `total` is 0, say the store is "
    "clean and stop; do not go looking for other work.\n\n"
    "Step 2 -- TRIAGE, worst first. Take the reasons in the order the tool "
    "returned them and work one GROUP at a time (all the `near_dup`s, then "
    "all the `status_fact`s, ...). Do not dump the whole list at once.\n\n"
    "For each suspect, before showing it to the user, look it up with "
    "`legend_recall` — a bare name and a reason code are not enough for "
    "anyone to judge. Read the FACTS that reference it, not just its summary: "
    "the entries most likely to be flagged are the ones that never had a "
    "summary, and what they mean lives entirely in the facts around them. "
    "Then give the user, in two or three lines: what the entry says, why the "
    "tool flagged it, and the single most likely repair. Then ask. One "
    "decision at a time.\n\n"
    "Expect false positives and say so plainly when you find one. If the "
    "surrounding facts show the entry is correct as it stands, tell the user "
    "it is fine and move on -- do not manufacture a repair to justify the "
    "flag.\n\n"
    "What each reason usually means, and the repair to propose:\n"
    "- `phantom_close`: a `resolves` names a target that never existed, so "
    "the real item is still open and a decoy sits beside it. Confirm which "
    "item they MEANT to close, then `retract` the bogus fact and re-save the "
    "resolves against the real one.\n"
    "- `status_fact`: a value that changes over time was written as a plain "
    "fact, so the old value stays live beside the new one. Ask what the "
    "current value is, then re-express it with `changes`; `retract` the "
    "stale fact.\n"
    "- `near_dup`: two names that may be one thing. Show BOTH summaries -- "
    "they are often genuinely different, and a wrong `merge` is hard to "
    "undo. Only on a clear yes, `merge` the later into the earlier.\n"
    "- `prose_name`: a sentence ended up as an element name (fact objects and "
    "attr values become names). Propose a short canonical name via "
    "`rename_to`, and offer to move the sentence into the summary.\n"
    "- `stale_open`: an open question or task nobody has touched. Ask: done, "
    "abandoned, or still live? If done, save a `resolves` fact from whatever "
    "settled it -- an existing element, never a new one. If still live, "
    "leave it and move on.\n"
    "- `orphan`: nothing references it. Usually harmless residue. Offer "
    "removal only if the user recognises it as a mistake.\n"
    "- `bloat`: a summary past ~400 chars, grown into a wall. Propose splitting the detail "
    "into child elements and leaving a short core, and show the proposed "
    "short core before writing it.\n\n"
    "Step 3 -- APPLY. Batch the approved repairs into as few `legend_save` "
    "calls as you can and set `source` to \"maintenance <today's date>\", so "
    "gardening stays distinguishable from real session work later. Check "
    "`writes.minted_elements` in every response: a name you expected to "
    "already exist appearing there means you just created a phantom -- "
    "retract it and tell the user.\n\n"
    "Step 4 -- STOP CLEANLY. Maintenance is interruptible by design. Whenever "
    "the user is done, summarise what changed and what is still outstanding. "
    "Do not press on through the whole list for completeness.\n\n"
    "Two standing rules. A suspect is a QUESTION, not a defect: the tool "
    "reports shapes, and only the user knows whether a given entry is wrong. "
    "And a memory that disagrees with the current code is not automatically "
    "stale -- it may be recording what was INTENDED, and that gap is worth "
    "more than a tidy store. Surface the disagreement; never silently correct "
    "it away.";

/* `prompts/get onboard` — the discovery script for starting Legend on an
 * EXISTING project: the client LLM explores the repo, interviews the user for
 * the why the code cannot tell, then seeds the store. Served over MCP prompts
 * (clients surface it as a slash command) so the flow travels with the binary
 * into any project `legend init` touches. */
static const char MCP_ONBOARD_PROMPT[] =
    "Onboard Legend onto this project by building a real ONTOLOGY of it, "
    "collaboratively. This is a deep session -- take the time it needs (an hour "
    "or two is fine). The goal is not to index the repo; it is to genuinely "
    "UNDERSTAND the project and lay down a small, intentional, well-connected "
    "graph -- including what is not written down anywhere.\n\n"
    "1 -- READ EVERYTHING, taking working notes as you go. Every doc, all the "
    "source, the configs, the full git history -- the whole AUTHORED project, "
    "not a curated subset (skip only generated code, dependencies, and build "
    "output -- no intent lives there). Keep a running SCRATCH FILE with four "
    "sections: ENTITIES (nouns), RELATIONSHIPS (facts between them), WHY (the "
    "reasoning behind what you see), and QUESTIONS FOR THE HUMAN (gaps + what "
    "you cannot infer). Your context window will not hold the whole project -- "
    "do not trust your memory, write it down as you read. This scratch is your "
    "draft ontology; do not touch Legend yet.\n\n"
    "2 -- MODEL THE ONTOLOGY from the full scratch, now that you have seen "
    "everything. An ENTITY is a short NOUN (<=5 words, most one or two) for one "
    "intentional thing. A CLAIM -- anything asserting something between entities "
    "-- is a RELATIONSHIP (a fact), never a name. Consolidate duplicates; "
    "sharpen the relationships.\n\n"
    "3 -- HUNT FOR THE WHY. The source shows WHAT exists; find the WHY -- why "
    "this architecture, why each decision, what was tried and rejected. Most of "
    "it is not written down. Record what you can infer under WHY, and what you "
    "cannot under QUESTIONS.\n\n"
    "4 -- FIND WHAT IS MISSING from the source: undocumented decisions, tacit "
    "conventions, known pitfalls, the intent behind the structure. This "
    "unwritten knowledge is the highest-value memory -- exactly what code and "
    "docs cannot hold. Add it to QUESTIONS.\n\n"
    "5 -- INTERVIEW THE HUMAN. Ask everything in QUESTIONS, a few at a time, as "
    "many rounds as it takes. Fold the answers back into the scratch.\n\n"
    "6 -- BUILD INTENTIONALLY into Legend, from the refined scratch. Reify the "
    "meaningful entities + durable facts + the why + the interview knowledge -- "
    "not every phrase. Names are short nouns; claims are facts between entities. "
    "A DECISION is a hub, not a headline: name it for the topic it settles and "
    "break out what it chose between -- chose/rejected point at the option "
    "entities and reason holds the why, so `Bolt as the default` becomes a "
    "decision `default fumble shape` with chose=Bolt, rejected=Melee, reason=..., "
    "never one opaque node (that leaves Bolt and Melee as nodes that never "
    "existed). "
    "Recall before each save and reuse canonical names; save in batches (project "
    "+ systems, then decisions, then constraints, then tasks/questions, then doc "
    "pointers); give everything a src pointer (file path or commit). After each "
    "batch check writes.minted_elements -- a prose or phantom name there means "
    "you named a claim; fix it with rename_to / merge / retract.\n\n"
    "DONE when legend_recall with no focus shows an orientation packet the human "
    "signs off on, and 2-3 focused recalls read true.";

/* value token index of member `key` in object `obj`; -1 if absent */
static long mcp_obj_get(const Json *j, u32 obj, const char *key) {
  u32 k, m, klen = (u32)strlen(key);
  if (j->toks[obj].type != J_OBJ)
    return -1;
  k = obj + 1;
  for (m = 0; m < j->toks[obj].size; m++) {
    u32 keyi = k, vali = k + 1; /* a key is a string: no subtree */
    const Tok *kt = &j->toks[keyi];
    if (kt->end - kt->start == klen &&
        memcmp(j->buf + kt->start, key, klen) == 0)
      return (long)vali;
    k = tok_skip(j->toks, vali);
  }
  return -1;
}

static int mcp_streq(const Json *j, long i, const char *s) {
  const Tok *t;
  u32 n;
  if (i < 0)
    return 0;
  t = &j->toks[i];
  n = t->end - t->start;
  return (u32)strlen(s) == n && memcmp(j->buf + t->start, s, n) == 0;
}

/* echo a request id verbatim: numbers/literals raw, strings re-quoted */
static void mcp_put_id(const Json *j, long id_i) {
  const Tok *t;
  if (id_i < 0) {
    fputs("null", stdout);
    return;
  }
  t = &j->toks[id_i];
  if (t->type == J_STR) {
    char tmp[256];
    u32 n = t->end - t->start;
    if (n > sizeof tmp - 1)
      n = sizeof tmp - 1;
    memcpy(tmp, j->buf + t->start, n);
    tmp[n] = 0;
    fputc('"', stdout);
    json_escape_fputs(tmp, stdout);
    fputc('"', stdout);
  } else {
    fwrite(j->buf + t->start, 1, t->end - t->start, stdout);
  }
}

/* frame capture: emit_frame() writes to stdout, which in server mode is the
 * protocol channel, so redirect fd 1 to a temp file around the call. */
static int mcp_cap_fd = -1;
static FILE *mcp_cap_fp = NULL;

static void mcp_cap_begin(void) {
  fflush(stdout);
  mcp_cap_fd = dup(1);
  mcp_cap_fp = tmpfile();
  if (mcp_cap_fp)
    dup2(fileno(mcp_cap_fp), 1);
}
static void mcp_cap_restore(void) {
  fflush(stdout);
  if (mcp_cap_fd >= 0) {
    dup2(mcp_cap_fd, 1);
    close(mcp_cap_fd);
    mcp_cap_fd = -1;
  }
}
static char *mcp_cap_take(void) { /* restore stdout; return malloc'd frame */
  char *out = NULL;
  long sz;
  mcp_cap_restore();
  if (mcp_cap_fp) {
    fseek(mcp_cap_fp, 0, SEEK_END);
    sz = ftell(mcp_cap_fp);
    fseek(mcp_cap_fp, 0, SEEK_SET);
    out = (char *)malloc((size_t)(sz < 0 ? 0 : sz) + 1);
    if (out) {
      size_t got = sz > 0 ? fread(out, 1, (size_t)sz, mcp_cap_fp) : 0;
      out[got] = 0;
    }
    fclose(mcp_cap_fp);
    mcp_cap_fp = NULL;
  }
  return out;
}
static void mcp_cap_discard(void) { /* error path: restore, drop capture */
  mcp_cap_restore();
  if (mcp_cap_fp) {
    fclose(mcp_cap_fp);
    mcp_cap_fp = NULL;
  }
}

static void mcp_reply_head(const Json *j, long id_i) {
  fputs("{\"jsonrpc\":\"2.0\",\"id\":", stdout);
  mcp_put_id(j, id_i);
}
static void mcp_initialize(const Json *j, long id_i) {
  mcp_reply_head(j, id_i);
  fputs(",\"result\":{\"protocolVersion\":\"2024-11-05\","
        "\"capabilities\":{\"tools\":{},\"prompts\":{}},"
        "\"serverInfo\":{\"name\":\"legend\",\"version\":\"2.0\"},"
        "\"instructions\":\"",
        stdout);
  json_escape_fputs(MCP_INSTRUCTIONS, stdout);
  fputs("\"}}\n", stdout);
  fflush(stdout);
}
static void mcp_tools_list(const Json *j, long id_i) {
  mcp_reply_head(j, id_i);
  fputs(",\"result\":{\"tools\":", stdout);
  fputs(MCP_TOOLS_JSON, stdout);
  fputs("}}\n", stdout);
  fflush(stdout);
}
static void mcp_empty_result(const Json *j, long id_i) {
  mcp_reply_head(j, id_i);
  fputs(",\"result\":{}}\n", stdout);
  fflush(stdout);
}
static void mcp_prompts_list(const Json *j, long id_i) {
  mcp_reply_head(j, id_i);
  fputs(",\"result\":{\"prompts\":[{\"name\":\"onboard\",\"description\":"
        "\"Start Legend on an existing project: explore the repo, interview "
        "the user for the why, seed the store, confirm the packet.\"},"
        "{\"name\":\"maintain\",\"description\":"
        "\"Garden the store with the user: audit for questionable entries, "
        "present them one at a time, and apply only what they approve.\"}]}}\n",
        stdout);
  fflush(stdout);
}
static void mcp_error(const Json *j, long id_i, int code, const char *msg) {
  mcp_reply_head(j, id_i);
  fprintf(stdout, ",\"error\":{\"code\":%d,\"message\":\"", code);
  json_escape_fputs(msg, stdout);
  fputs("\"}}\n", stdout);
  fflush(stdout);
}
static void mcp_prompts_get(const Json *j, long id_i) {
  long params_i = mcp_obj_get(j, 0, "params");
  long name_i = params_i >= 0 ? mcp_obj_get(j, (u32)params_i, "name") : -1;
  const char *text;
  if (mcp_streq(j, name_i, "onboard"))
    text = MCP_ONBOARD_PROMPT;
  else if (mcp_streq(j, name_i, "maintain"))
    text = MCP_MAINTAIN_PROMPT;
  else {
    mcp_error(j, id_i, -32602, "unknown prompt");
    return;
  }
  mcp_reply_head(j, id_i);
  fputs(",\"result\":{\"messages\":[{\"role\":\"user\",\"content\":"
        "{\"type\":\"text\",\"text\":\"",
        stdout);
  json_escape_fputs(text, stdout);
  fputs("\"}}]}}\n", stdout);
  fflush(stdout);
}
static void mcp_tool_result(const Json *j, long id_i, const char *text,
                            int is_error) {
  mcp_reply_head(j, id_i);
  fputs(",\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"", stdout);
  json_escape_fputs(text, stdout);
  fputs("\"}]", stdout);
  if (is_error)
    fputs(",\"isError\":true", stdout);
  fputs("}}\n", stdout);
  fflush(stdout);
}

/* ---- warm-graph gate ---------------------------------------------------- *
 * The server keeps g_graph in memory across calls and reloads only when the
 * on-disk snapshot changed under it. The per-call flock serializes writers, so
 * a fingerprint taken under the lock cannot miss an external write; our own
 * writes re-fingerprint so we never reload our own change. */
static int g_graph_warm = 0;
static i64 g_snap_size = -1, g_snap_mtime = 0, g_snap_mtime_ns = 0;

static int snapshot_fingerprint(const char *store, i64 *size, i64 *mtime,
                                i64 *ns) {
  char path[4400];
  struct stat st;
  snprintf(path, sizeof path, "%s/legend.snapshot", store);
  if (stat(path, &st) != 0)
    return 0;
  *size = (i64)st.st_size;
  *mtime = (i64)st.st_mtim.tv_sec;
  *ns = (i64)st.st_mtim.tv_nsec;
  return 1;
}

/* Record that g_graph matches the on-disk snapshot — after a startup load or
 * our own write — so the next gate does not reload what we just wrote. */
static void graph_mark_synced(const char *store) {
  g_graph_warm =
      snapshot_fingerprint(store, &g_snap_size, &g_snap_mtime, &g_snap_mtime_ns);
}

static int graph_traced(void) {
  static int on = -1;
  if (on < 0) {
    const char *t = getenv("LEGEND_TRACE");
    on = (t && strcmp(t, "0") != 0);
  }
  return on;
}

/* Reload g_graph only when the snapshot changed on disk (or a prior call left
 * it dirty). snapshot_load appends, so a reload resets the graph first. */
/* The `build` of the last journal line, or "" when the store has no journal.
 * The tail is enough: lines are appended and the newest is last. */
static void journal_last_build(const char *store, char *out, size_t outn) {
  char path[4300], tail[8192];
  FILE *f;
  long size;
  size_t got, i;
  static const char KEY[] = "\"build\":\"";
  out[0] = '\0';
  snprintf(path, sizeof path, "%s/journal.jsonl", store);
  f = fopen(path, "rb");
  if (!f)
    return;
  if (fseek(f, 0, SEEK_END) != 0 || (size = ftell(f)) < 0) {
    fclose(f);
    return;
  }
  if ((size_t)size > sizeof tail)
    fseek(f, size - (long)sizeof tail, SEEK_SET);
  else
    fseek(f, 0, SEEK_SET);
  got = fread(tail, 1, sizeof tail, f);
  fclose(f);
  for (i = got; i-- > 0;) {
    if (i + sizeof KEY - 1 > got || memcmp(tail + i, KEY, sizeof KEY - 1) != 0)
      continue;
    {
      const char *v = tail + i + sizeof KEY - 1;
      size_t n = 0;
      while (v + n < tail + got && v[n] != '"' && n + 1 < outn)
        n++;
      memcpy(out, v, n);
      out[n] = '\0';
    }
    return;
  }
}

static void graph_sync(const char *store) {
  i64 size = -1, mtime = 0, ns = 0;
  int present = snapshot_fingerprint(store, &size, &mtime, &ns);
  int was_warm = g_graph_warm;
  if (g_graph_warm && present && size == g_snap_size && mtime == g_snap_mtime &&
      ns == g_snap_mtime_ns) {
    if (graph_traced())
      fprintf(stderr, "[graph] warm hit (%u elements)\n", g_graph.element_count);
    return; /* unchanged: keep the warm graph */
  }
  /* Reloading a graph we already held warm means another process wrote. A
   * writer on a different build is a re-pin that left this server behind (or
   * left another one behind), which the journal stamps but nothing surfaces. */
  if (was_warm) {
    char last[24];
    journal_last_build(store, last, sizeof last);
    if (last[0] && strcmp(last, LEGEND_BUILD) != 0)
      snprintf(g_foreign_build, sizeof g_foreign_build, "%s", last);
  }
  if (graph_traced())
    fprintf(stderr, "[graph] reload (%s)\n",
            g_graph_warm ? "snapshot changed on disk" : "cold or dirty");
  graph_free(&g_graph);
  if (!snapshot_load(&g_graph, store))
    fail(ERR_NO_STORE, NULL, "no snapshot in %s", store);
  g_snap_size = size;
  g_snap_mtime = mtime;
  g_snap_mtime_ns = ns;
  g_graph_warm = 1;
}

static void mcp_tools_call(const Json *j, long id_i, const char *store,
                           const char *pristine) {
  long params_i = mcp_obj_get(j, 0, "params");
  long name_i = params_i >= 0 ? mcp_obj_get(j, (u32)params_i, "name") : -1;
  long args_i = params_i >= 0 ? mcp_obj_get(j, (u32)params_i, "arguments") : -1;
  int is_save, is_recall, is_audit;
  if (name_i < 0) {
    mcp_error(j, id_i, -32602, "missing tool name");
    return;
  }
  is_save = mcp_streq(j, name_i, "legend_save");
  is_recall = mcp_streq(j, name_i, "legend_recall");
  is_audit = mcp_streq(j, name_i, "legend_audit");
  if (!is_save && !is_recall && !is_audit) {
    mcp_error(j, id_i, -32602, "unknown tool");
    return;
  }
  /* audit takes nothing, and clients differ on whether a no-arg call carries
   * an empty `arguments` object at all */
  if (args_i < 0 && !is_audit) {
    mcp_error(j, id_i, -32602, "missing arguments");
    return;
  }
  g_err_trap = 1;
  if (setjmp(g_err_jmp) == 0) {
    static Submission sub;
    static Recall rec;
    static WriteReport report;
    Rd rd;
    char *frame;
    if (args_i >= 0) {
      rd.t = &j->toks[args_i]; /* Rd navigates relative to its root token */
      rd.buf = j->buf;
    }
    acquire_lock(store); /* brief per-call hold, released before we reply */
    if (is_audit) {
      /* Deliberately unjournaled. The journal exists to replay mutations, and
       * replay_journal.py re-runs every ok entry as `legend <verb> <payload>`
       * — a payload-less read would break the replay the trial's determinism
       * check depends on. Nothing is written, so there is nothing to replay.
       * (journal_append below is a no-op while unarmed.) */
      graph_sync(store);
      mcp_cap_begin();
      audit_graph(&g_graph, args_i >= 0 ? read_audit_limit(&rd)
                                        : (i64)g_aud_per_reason);
    } else {
      /* container-token start/end are original read offsets, so the slice of
       * the pristine line is the arguments subtree exactly as submitted */
      journal_arm(store, is_save ? "save" : "recall",
                  pristine + j->toks[args_i].start,
                  j->toks[args_i].end - j->toks[args_i].start);
      graph_sync(store); /* warm: reload only when the snapshot changed on disk */
      if (is_save) {
        read_submission(&rd, &sub);
        mcp_cap_begin();
        tick_save(&g_graph, &sub, j->buf, &report);
        snapshot_write(&g_graph, store);
        graph_mark_synced(store); /* g_graph now matches what we just wrote */
        tier2_sync(&g_graph);
        emit_frame(&g_graph, &report, store, 40, 2, -1);
      } else {
        read_recall(&rd, &rec);
        g_journal_observe = rec.observe;
        mcp_cap_begin();
        tick_recall(&g_graph, &rec, j->buf, &report);
        if (!rec.observe) {
          snapshot_write(&g_graph, store);
          graph_mark_synced(store);
        }
        emit_frame(&g_graph, &report, store, rec.limit, rec.history_depth,
                   rec.since);
      }
    }
    frame = mcp_cap_take();
    if (is_recall)
      g_journal_bytes_out = frame ? (i64)strlen(frame) : 0;
    journal_append(report.at_secs, 1, -1);
    release_lock();
    g_err_trap = 0;
    mcp_tool_result(j, id_i, frame ? frame : "{}", 0);
    free(frame);
  } else {
    char msg[600];
    g_err_trap = 0;
    g_graph_warm = 0; /* a fail() may have left g_graph half-mutated: reload next call */
    mcp_cap_discard();
    journal_append((i64)time(NULL), 0, g_err.code);
    release_lock(); /* a fail() may have longjmp'd while we held the lock */
    snprintf(msg, sizeof msg, "%s%s%s", g_err.message,
             g_err.at[0] ? " at " : "", g_err.at);
    mcp_tool_result(j, id_i, msg, 1);
  }
}

static void mcp_handle(char *buf, u32 len, const char *store) {
  static Json j;
  static char *pristine; /* the line before in-place unescaping (journal) */
  static size_t pristine_cap;
  long method_i, id_i;
  if (pristine_cap < (size_t)len + 1) {
    free(pristine);
    pristine_cap = (size_t)len + 1;
    pristine = (char *)malloc(pristine_cap);
    if (!pristine) {
      fprintf(stderr, "legend: out of memory\n");
      exit(1);
    }
  }
  memcpy(pristine, buf, (size_t)len + 1);
  g_err_trap = 1;
  if (setjmp(g_err_jmp)) { /* json_parse rejected the line */
    g_err_trap = 0;
    fputs("{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32700,"
          "\"message\":\"parse error\"}}\n",
          stdout);
    fflush(stdout);
    return;
  }
  json_parse(&j, buf, len);
  g_err_trap = 0;
  if (j.count == 0 || j.toks[0].type != J_OBJ)
    return;
  method_i = mcp_obj_get(&j, 0, "method");
  id_i = mcp_obj_get(&j, 0, "id");
  if (method_i < 0)
    return; /* a response, not a request */
  if (mcp_streq(&j, method_i, "initialize"))
    mcp_initialize(&j, id_i);
  else if (mcp_streq(&j, method_i, "tools/list"))
    mcp_tools_list(&j, id_i);
  else if (mcp_streq(&j, method_i, "tools/call"))
    mcp_tools_call(&j, id_i, store, pristine);
  else if (mcp_streq(&j, method_i, "prompts/list"))
    mcp_prompts_list(&j, id_i);
  else if (mcp_streq(&j, method_i, "prompts/get"))
    mcp_prompts_get(&j, id_i);
  else if (mcp_streq(&j, method_i, "ping"))
    mcp_empty_result(&j, id_i);
  else if (id_i >= 0)
    mcp_error(&j, id_i, -32601, "method not found");
  /* else: a notification (no id) we do not handle -- stay silent */
}

static int mcp_serve(void) {
  static char store[4200];
  char *line = NULL;
  size_t cap = 0;
  ssize_t n;
  if (!discover_store(store, sizeof store))
    fail(ERR_NO_STORE, NULL,
         "no .legend store found (set LEGEND_STATE_DIR or run legend init)");
  if (!snapshot_load(&g_graph, store))
    fail(ERR_NO_STORE, NULL, "no snapshot in %s -- run legend init", store);
  graph_mark_synced(store); /* fingerprint the startup load: g_graph is warm */
  embed_warm();             /* load model + sidecar now, off the first call's path */
  /* the lock is taken per tools/call (see mcp_tools_call), not for the whole
   * session, so a SessionStart hook's `legend recall` never blocks on us */
  while ((n = getline(&line, &cap, stdin)) > 0) {
    u32 len = (u32)n;
    while (len && (line[len - 1] == '\n' || line[len - 1] == '\r'))
      len--;
    if (len == 0)
      continue;
    line[len] = 0;
    mcp_handle(line, len, store); /* parses `line` in place */
  }
  free(line);
  return 0;
}

LEGEND_UNUSED static int run_cli(int argc, char **argv) {
  const char *verb = NULL, *inline_payload = NULL;
  int pretty = 0, reset = 0, i;
  /* a truncating consumer (a hook's `head -c`) closing stdout must not kill
   * the process — writes fail with EPIPE instead and the invocation finishes */
  signal(SIGPIPE, SIG_IGN);
  for (i = 1; i < argc; i++) {
    const char *a = argv[i];
    if (strcmp(a, "--pretty") == 0)
      pretty = 1;
    else if (strcmp(a, "--reset") == 0)
      reset = 1;
    else if (a[0] == '-' && a[1] != 0)
      fail(ERR_PARSE, NULL, "unknown flag %s", a);
    else if (!verb)
      verb = a;
    else if (!inline_payload)
      inline_payload = a;
    else
      fail(ERR_PARSE, NULL, "unexpected argument %s", a);
  }
  g_pretty = pretty;
  if (!verb)
    fail(ERR_PARSE, NULL,
         "usage: legend save|recall|init|dump|audit|mcp-serve [payload] "
         "[--pretty] [--reset]");
  if (strcmp(verb, "init") == 0) {
    if (inline_payload)
      fail(ERR_PARSE, NULL, "init takes no payload");
    return cmd_init(reset);
  }
  if (strcmp(verb, "audit") == 0) {
    static char store[4200];
    static Json json;
    i64 limit = (i64)g_aud_per_reason;
    if (!discover_store(store, sizeof store))
      fail(ERR_NO_STORE, NULL,
           "no .legend store found (set LEGEND_STATE_DIR or run legend init)");
    if (inline_payload) {
      Rd rd;
      u32 len = payload_from_arg(inline_payload);
      json_parse(&json, g_payload, len);
      rd.t = json.toks;
      rd.buf = g_payload;
      limit = read_audit_limit(&rd);
    }
    if (!snapshot_load(&g_graph, store))
      fail(ERR_NO_STORE, NULL, "no snapshot in %s — run legend init", store);
    audit_graph(&g_graph, limit);
    return 0;
  }
  if (strcmp(verb, "dump") == 0) {
    static char store[4200];
    if (inline_payload)
      fail(ERR_PARSE, NULL, "dump takes no payload");
    if (!discover_store(store, sizeof store))
      fail(ERR_NO_STORE, NULL,
           "no .legend store found (set LEGEND_STATE_DIR or run legend init)");
    if (!snapshot_load(&g_graph, store))
      fail(ERR_NO_STORE, NULL, "no snapshot in %s — run legend init", store);
    dump_graph(&g_graph);
    return 0;
  }
  if (strcmp(verb, "mcp-serve") == 0) {
    if (inline_payload)
      fail(ERR_PARSE, NULL, "mcp-serve takes no payload");
    return mcp_serve();
  }
  if (reset)
    fail(ERR_PARSE, NULL, "--reset applies to init only");
  if (strcmp(verb, "save") != 0 && strcmp(verb, "recall") != 0)
    fail(ERR_PARSE, NULL,
         "unknown verb %s; expected save, recall, init, dump, or audit", verb);

  {
    static char store[4200];
    static Json json;
    static Submission sub;
    static Recall rec;
    Rd rd;
    u32 payload_len;
    if (!discover_store(store, sizeof store))
      fail(ERR_NO_STORE, NULL,
           "no .legend store found (set LEGEND_STATE_DIR or run legend init)");
    /* Payload is read before the lock: never hold the lock while blocked
     * on a caller's pipe. */
    payload_len = inline_payload ? payload_from_arg(inline_payload)
                                 : payload_from_stdin();
    acquire_lock(store);
    journal_arm(store, verb, g_payload_orig, payload_len);
    json_parse(&json, g_payload, payload_len);
    rd.t = json.toks;
    rd.buf = g_payload;
    if (strcmp(verb, "save") == 0) {
      static WriteReport report;
      read_submission(&rd, &sub);
      sweep_orphan_tmps(store);
      if (!snapshot_load(&g_graph, store))
        /* only init creates a store (spec §4): a dir with no snapshot is not
         * one */
        fail(ERR_NO_STORE, NULL, "no snapshot in %s — run legend init", store);
      tick_save(&g_graph, &sub, g_payload, &report);
      snapshot_write(&g_graph, store);
      tier2_sync(&g_graph); /* eager: keep the vector sidecar warm per save */
      /* journal BEFORE the frame: the journal records the applied mutation,
       * and a consumer that truncates the frame (a hook's `head -c`) can
       * kill this process mid-emit — the store must never move unjournaled */
      journal_append(report.at_secs, 1, -1);
      emit_frame(&g_graph, &report, store, 40, 2, -1);
      return 0;
    }
    read_recall(&rd, &rec);
    g_journal_observe = rec.observe;
    {
      static WriteReport report;
      sweep_orphan_tmps(store);
      if (!snapshot_load(&g_graph, store))
        fail(ERR_NO_STORE, NULL, "no snapshot in %s — run legend init", store);
      tick_recall(&g_graph, &rec, g_payload, &report);
      if (!rec.observe)
        snapshot_write(&g_graph, store);
      /* journal BEFORE the frame (durability, see the save path above); buffer
       * the compact frame first so bytes_out records the emitted size without
       * breaking the journal-before-emit invariant. --pretty is a human
       * surface, not the token-cost path, so it streams as before. */
      if (g_pretty) {
        journal_append(report.at_secs, 1, -1);
        emit_frame(&g_graph, &report, store, rec.limit, rec.history_depth,
                   rec.since);
      } else {
        u32 flen;
        char *fbuf = capture_frame_json(&g_graph, &report, store, rec.limit,
                                        rec.history_depth, rec.since, &flen);
        g_journal_bytes_out = (i64)flen;
        journal_append(report.at_secs, 1, -1);
        fwrite(fbuf, 1, flen, stdout);
        free(fbuf);
      }
      return 0;
    }
  }
}

#ifndef LEGEND_NO_MAIN
int main(int argc, char **argv) { return run_cli(argc, argv); }
#endif
