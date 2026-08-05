/* legend_test.c — WP0 unit tests for legend.c (the stb/SQLite include pattern).
 *
 * Build: cc -std=c99 -Wall -Wextra -Werror legend_test.c -o legend_test
 * Every test that touches a store uses its own mkdtemp() dir via
 * LEGEND_STATE_DIR; no test ever shares a store (plan §1).
 */

#define LEGEND_NO_MAIN
#include "legend.c"

static int t_checks, t_fails;

#define CHECK(cond) do { \
    t_checks++; \
    if (!(cond)) { t_fails++; printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); } \
} while (0)

/* Run stmt with fail() trapped; failed_out = 1 iff it failed. */
#define TRY(stmt, failed_out) do { \
    g_err_trap = 1; \
    if (setjmp(g_err_jmp) == 0) { stmt; (failed_out) = 0; } \
    else (failed_out) = 1; \
    g_err_trap = 0; \
} while (0)

/* Shared fixtures: static so partially-filled state stays reachable (and
 * leak-clean under ASan) when a trapped fail() longjmps out mid-parse. */
static char       tb[LEGEND_PAYLOAD_CAP + 2];
static Json       tj;
static Submission tsub;
static Recall     trec;

static int span_eq(const char *buf, Span s, const char *want) {
    return !span_absent(s) && strlen(want) == s.len && memcmp(buf + s.off, want, s.len) == 0;
}

/* ------------------------------ S1 tests ------------------------------ */

static void expect_norm(const char *in, const char *want) {
    char out[256];
    u32 n = normalize_name(in, (u32)strlen(in), out);
    t_checks++;
    if (n != strlen(want) || memcmp(out, want, n) != 0) {
        t_fails++;
        printf("FAIL normalize(\"%s\") -> \"%.*s\", want \"%s\"\n", in, (int)n, out, want);
    }
}

static void test_normalize(void) {
    expect_norm("Jump Physics", "jump physics");
    expect_norm("jump-physics", "jump physics");
    expect_norm("jump_physics", "jump physics");
    expect_norm("JUMP", "jump");
    expect_norm("  hello   world  ", "hello world");
    expect_norm("A--B__C", "a b c");
    expect_norm("don't", "don t");
    expect_norm("what?!", "what");
    expect_norm("(a)", "a");
    expect_norm("a\tb\nc", "a b c");
    expect_norm("x\vy\fz", "x y z");
    expect_norm("K[L]M", "k[l]m"); /* only A-Z folds; neighbors of the range don't */
    /* UTF-8 passes through byte-for-byte; only ASCII A-Z folds */
    expect_norm("Caf\xC3\xA9-Cr\xC3\xA8me", "caf\xC3\xA9 cr\xC3\xA8me");
    expect_norm("\xC3\x9C" "BER", "\xC3\x9C" "ber");
    /* empty after normalize — the caller raises parse on this */
    expect_norm("---", "");
    expect_norm("", "");
    expect_norm(".,;:!?'\"()", "");
}

static void test_trigrams(void) {
    U32Vec a = { 0, 0, 0 }, b = { 0, 0, 0 };

    trigram_set_build("abcd", 4, &a);
    CHECK(a.count == 2 && a.v[0] == 0x616263u && a.v[1] == 0x626364u);

    trigram_set_build("dcba", 4, &a); /* output is sorted */
    CHECK(a.count == 2 && a.v[0] == 0x636261u && a.v[1] == 0x646362u);

    trigram_set_build("aaaa", 4, &a); /* and unique */
    CHECK(a.count == 1 && a.v[0] == 0x616161u);

    trigram_set_build("ab", 2, &a); /* short strings yield one partial window */
    CHECK(a.count == 1 && a.v[0] == 0x616200u);
    trigram_set_build("a", 1, &a);
    CHECK(a.count == 1 && a.v[0] == 0x610000u);
    trigram_set_build("", 0, &a);
    CHECK(a.count == 0);

    trigram_set_build("abcd", 4, &a);
    trigram_set_build("abcd", 4, &b);
    CHECK(trigram_jaccard(a.v, a.count, b.v, b.count) == 1.0);

    trigram_set_build("abc", 3, &a);
    trigram_set_build("abd", 3, &b);
    CHECK(trigram_jaccard(a.v, a.count, b.v, b.count) == 0.0);

    trigram_set_build("abcd", 4, &a); /* {abc,bcd} vs {bcd,cde}: 1 of 3 */
    trigram_set_build("bcde", 4, &b);
    {
        double j = trigram_jaccard(a.v, a.count, b.v, b.count);
        CHECK(j > 0.333332 && j < 0.333334);
    }

    CHECK(trigram_jaccard(NULL, 0, NULL, 0) == 0.0);

    free(a.v);
    free(b.v);
}

static void test_str_arena(void) {
    StrArena a;
    memset(&a, 0, sizeof a);

    CHECK(str_find(&a, "foo", 3) == NONE_U32); /* find on empty never mutates */
    CHECK(a.count == 0);

    CHECK(str_intern(&a, "foo", 3) == 0);
    CHECK(str_find(&a, "foo", 3) == 0);
    CHECK(str_intern(&a, "foo", 3) == 0); /* intern of existing reuses */
    CHECK(a.count == 1);
    CHECK(str_intern(&a, "bar", 3) == 1);
    CHECK(str_find(&a, "baz", 3) == NONE_U32);
    CHECK(a.count == 2); /* the miss did not mutate */
    CHECK(str_len(&a, 0) == 3 && memcmp(str_ptr(&a, 0), "foo", 3) == 0);
    CHECK(str_ptr(&a, 0)[3] == 0); /* NUL-terminated backing bytes */

    /* growth: force bytes, records, and slot-table rehashing */
    {
        u32 i;
        char name[32];
        for (i = 0; i < 1000; i++) {
            snprintf(name, sizeof name, "s%u", i);
            CHECK(str_intern(&a, name, (u32)strlen(name)) == 2 + i);
        }
        CHECK(a.count == 1002);
        for (i = 0; i < 1000; i++) {
            snprintf(name, sizeof name, "s%u", i);
            CHECK(str_find(&a, name, (u32)strlen(name)) == 2 + i);
        }
    }
    str_arena_free(&a);
    CHECK(a.count == 0 && a.bytes == NULL);
}

static void test_str_map(void) {
    StrArena a;
    StrMap m;
    u32 i;
    char key[32];
    memset(&a, 0, sizeof a);
    memset(&m, 0, sizeof m);

    CHECK(smap_get(&m, &a, "k0", 2) == NONE_U32);
    for (i = 0; i < 500; i++) {
        snprintf(key, sizeof key, "k%u", i);
        smap_put(&m, &a, str_intern(&a, key, (u32)strlen(key)), i * 7);
    }
    CHECK(m.used == 500);
    for (i = 0; i < 500; i++) {
        snprintf(key, sizeof key, "k%u", i);
        CHECK(smap_get(&m, &a, key, (u32)strlen(key)) == i * 7);
    }
    CHECK(smap_get(&m, &a, "missing", 7) == NONE_U32);

    /* overwrite keeps used stable */
    smap_put(&m, &a, str_intern(&a, "k42", 3), 99999);
    CHECK(m.used == 500);
    CHECK(smap_get(&m, &a, "k42", 3) == 99999);

    smap_free(&m);
    str_arena_free(&a);
}

/* ------------------------------ S2 tests ------------------------------ */

static void toks(const char *src) {
    size_t n = strlen(src);
    memcpy(tb, src, n + 1);
    json_parse(&tj, tb, (u32)n);
}

static int tok_bytes(u32 ti, const char *want) {
    u32 n = tj.toks[ti].end - tj.toks[ti].start;
    return strlen(want) == n && memcmp(tb + tj.toks[ti].start, want, n) == 0;
}

static void expect_json_err(const char *src) {
    int failed;
    TRY(toks(src), failed);
    t_checks++;
    if (!failed || g_err.code != ERR_PARSE) {
        t_fails++;
        printf("FAIL expected parse error for %.60s\n", src);
    }
}

static void test_tokenizer(void) {
    int failed;

    toks("{\"a\":1}");
    CHECK(tj.count == 3);
    CHECK(tj.toks[0].type == J_OBJ && tj.toks[0].size == 1);
    CHECK(tj.toks[1].type == J_STR && tok_bytes(1, "a"));
    CHECK(tj.toks[2].type == J_NUM && tok_bytes(2, "1"));

    /* escapes, unescaped in place */
    toks("\"a\\\"b\\\\c\\/d\\b\\f\\n\\r\\t\"");
    CHECK(tj.toks[0].type == J_STR && tok_bytes(0, "a\"b\\c/d\b\f\n\r\t"));

    /* \uXXXX to UTF-8: 1-, 2-, 3-byte, and a surrogate pair (4-byte) */
    toks("\"\\u0041\"");
    CHECK(tok_bytes(0, "A"));
    toks("\"\\u00e9\"");
    CHECK(tok_bytes(0, "\xC3\xA9"));
    toks("\"\\u20ac\"");
    CHECK(tok_bytes(0, "\xE2\x82\xAC"));
    toks("\"\\ud83d\\ude00\"");
    CHECK(tok_bytes(0, "\xF0\x9F\x98\x80"));
    toks("\"x\\ud83d\\ude00y\"");
    CHECK(tok_bytes(0, "x\xF0\x9F\x98\x80y"));

    /* nesting + sizes + tok_skip */
    toks("[[1,2],{\"k\":[true,false,null]}]");
    CHECK(tj.toks[0].type == J_ARR && tj.toks[0].size == 2);
    CHECK(tj.toks[1].type == J_ARR && tj.toks[1].size == 2);
    CHECK(tok_skip(tj.toks, 1) == 4);
    CHECK(tj.toks[4].type == J_OBJ && tj.toks[4].size == 1);
    CHECK(tj.toks[6].type == J_ARR && tj.toks[6].size == 3);
    CHECK(tok_skip(tj.toks, 0) == tj.count);

    /* numbers */
    toks("[0,-1,3.14,-0.5e+2,1e10]");
    CHECK(tj.toks[0].size == 5 && tok_bytes(3, "3.14") && tok_bytes(4, "-0.5e+2"));

    /* truncated input */
    expect_json_err("");
    expect_json_err("   ");
    expect_json_err("{");
    expect_json_err("{\"a\"");
    expect_json_err("{\"a\":");
    expect_json_err("[1,2");
    expect_json_err("\"abc");
    expect_json_err("\"abc\\");
    expect_json_err("tru");

    /* trailing garbage */
    expect_json_err("{} {}");
    expect_json_err("1 2");
    expect_json_err("nullx");

    /* bad strings */
    expect_json_err("\"\\q\"");
    expect_json_err("\"a\nb\"");        /* raw control char */
    expect_json_err("\"\\ud800x\"");    /* lone high surrogate */
    expect_json_err("\"\\udc00\"");     /* lone low surrogate */
    expect_json_err("\"\\ud83dx\"");    /* high surrogate, no pair */
    expect_json_err("\"\\u12g4\"");

    /* bad numbers and structure */
    expect_json_err("01");
    expect_json_err("-");
    expect_json_err("1.");
    expect_json_err(".5");
    expect_json_err("+1");
    expect_json_err("{\"a\":1,}");
    expect_json_err("[1,]");
    expect_json_err("{'a':1}");
    expect_json_err("{\"a\" 1}");

    /* nesting depth cap */
    {
        u32 i;
        for (i = 0; i < 200; i++) tb[i] = '[';
        tb[200] = 0;
        g_err_trap = 1;
        if (setjmp(g_err_jmp) == 0) {
            json_parse(&tj, tb, 200);
            failed = 0;
        } else {
            failed = 1;
        }
        g_err_trap = 0;
        CHECK(failed && g_err.code == ERR_PARSE);
    }
}

static void parse_sub(const char *src) {
    Rd r;
    toks(src);
    r.t = tj.toks;
    r.buf = tb;
    read_submission(&r, &tsub);
}

static void parse_rec(const char *src) {
    Rd r;
    toks(src);
    r.t = tj.toks;
    r.buf = tb;
    read_recall(&r, &trec);
}

/* ---- M5 fuzz regression: the payload UTF-8 door ----
 * fuzz/fuzz_payload.py minted element names carrying raw invalid UTF-8; the
 * frame writer echoed the bytes and the frame stopped being valid JSON text.
 * String tokens now validate as UTF-8 (RFC 3629), so graph strings are valid
 * by construction. */
static void test_utf8_payload_door(void) {
    /* the directed find, byte for byte */
    expect_json_err("{\"elements\":[{\"name\":\"bad\xFF\xFEname\"}]}");

    /* one rejection per RFC 3629 rule */
    expect_json_err("\"a\xA4z\"");            /* bare continuation byte */
    expect_json_err("\"\xC0\xAF\"");          /* overlong 2-byte */
    expect_json_err("\"\xC1\xBF\"");          /* overlong lead */
    expect_json_err("\"\xC2x\"");             /* broken continuation */
    expect_json_err("\"\xE0\x80\x80\"");      /* overlong 3-byte */
    expect_json_err("\"\xED\xA0\x80\"");      /* surrogate range */
    expect_json_err("\"\xE2\x82\"");          /* truncated 3-byte */
    expect_json_err("\"\xF0\x80\x80\x80\"");  /* overlong 4-byte */
    expect_json_err("\"\xF4\x90\x80\x80\"");  /* beyond U+10FFFF */
    expect_json_err("\"\xF5\x80\x80\x80\"");  /* invalid lead byte */
    expect_json_err("\"caf\xC3\"");           /* cut at end of string */

    /* valid multibyte passes through untouched */
    toks("\"caf\xC3\xA9 \xE2\x82\xAC \xF0\x9F\x98\x80\"");
    CHECK(tok_bytes(0, "caf\xC3\xA9 \xE2\x82\xAC \xF0\x9F\x98\x80"));

    /* bounded escaping truncates by whole sequences, never mid-char */
    {
        char dst[8];
        u32 w = json_escape_buf(dst, 4, "caf\xC3\xA9", 5);
        CHECK(w == 3 && memcmp(dst, "caf", 3) == 0);
        w = json_escape_buf(dst, 8, "\xE2\x82", 2); /* pre-cut input: dropped */
        CHECK(w == 0);
    }
}

static void expect_sub_err(const char *payload, int want_code, const char *want_at) {
    int failed;
    TRY(parse_sub(payload), failed);
    CHECK(failed);
    if (!failed) return;
    t_checks++;
    if (g_err.code != want_code) {
        t_fails++;
        printf("FAIL code %s want %s (payload %.60s)\n",
               ERR_CODE_NAMES[g_err.code], ERR_CODE_NAMES[want_code], payload);
    }
    if (want_at) {
        t_checks++;
        if (strcmp(g_err.at, want_at) != 0) {
            t_fails++;
            printf("FAIL at \"%s\" want \"%s\" (payload %.60s)\n", g_err.at, want_at, payload);
        }
    }
}

static void expect_rec_err(const char *payload, int want_code, const char *want_at) {
    int failed;
    TRY(parse_rec(payload), failed);
    CHECK(failed);
    if (!failed) return;
    CHECK(g_err.code == want_code);
    if (want_at) {
        t_checks++;
        if (strcmp(g_err.at, want_at) != 0) {
            t_fails++;
            printf("FAIL at \"%s\" want \"%s\" (payload %.60s)\n", g_err.at, want_at, payload);
        }
    }
}

/* The attr value for key, when it has exactly one value. */
static Span attr_single(const SubElement *e, const char *key) {
    u32 i;
    for (i = 0; i < e->attr_count; i++) {
        const AttrEntry *a = &tsub.attr_pool[e->attr_start + i];
        if (span_eq(tb, a->key, key) && a->val_count == 1)
            return tsub.span_pool[a->val_start];
    }
    return span_none();
}

/* The spec §5 example payload, verbatim. */
static const char SPEC5_PAYLOAD[] =
"{\n"
"  \"source\": \"claude-code:platformer\",\n"
"  \"elements\": [\n"
"    { \"name\": \"coyote_time\", \"kind\": \"mechanic\", \"aliases\": [\"coyote frames\"],\n"
"      \"summary\": \"grace window after leaving a ledge in which a jump still fires\" },\n"
"    { \"name\": \"ground check via raycast\", \"kind\": \"decision\", \"salience\": 0.8,\n"
"      \"attrs\": { \"chose\": \"raycast ground check\", \"rejected\": \"capsule cast\",\n"
"                 \"reason\": \"cheaper per frame\", \"about\": \"player_jump\",\n"
"                 \"resolves\": \"ground check: raycast or capsule?\" } }\n"
"  ],\n"
"  \"facts\": [\n"
"    { \"s\": \"player_jump\", \"p\": \"uses\", \"o\": \"coyote_time\", \"confidence\": 0.9 },\n"
"    { \"s\": \"beta_release\", \"p\": \"at\", \"o\": \"August 2026\", \"status\": \"defeasible\", \"confidence\": 0.6 }\n"
"  ],\n"
"  \"changes\": [\n"
"    { \"target\": \"jump_height\", \"property\": \"value\", \"to\": \"4.2\", \"intervened\": true,\n"
"      \"src\": \"src/player/jump.rs:142\" }\n"
"  ],\n"
"  \"retract\": [\"rel:502\"],\n"
"  \"focus\": [\"jump feel\"]\n"
"}\n";

static void test_reader_spec_example(void) {
    int failed;
    TRY(parse_sub(SPEC5_PAYLOAD), failed);
    CHECK(!failed);
    if (failed) return;

    CHECK(span_eq(tb, tsub.source, "claude-code:platformer"));

    CHECK(tsub.element_count == 2);
    {
        const SubElement *e0 = &tsub.elements[0], *e1 = &tsub.elements[1];
        CHECK(span_eq(tb, e0->name, "coyote_time"));
        CHECK(span_eq(tb, e0->kind, "mechanic"));
        CHECK(e0->alias_count == 1 && span_eq(tb, tsub.span_pool[e0->alias_start], "coyote frames"));
        CHECK(span_eq(tb, e0->summary, "grace window after leaving a ledge in which a jump still fires"));
        CHECK(!e0->has_salience && e0->attr_count == 0 && !e0->is_new);

        CHECK(span_eq(tb, e1->name, "ground check via raycast"));
        CHECK(span_eq(tb, e1->kind, "decision"));
        CHECK(e1->has_salience && e1->salience == 0.8);
        CHECK(e1->attr_count == 5);
        CHECK(span_eq(tb, attr_single(e1, "chose"), "raycast ground check"));
        CHECK(span_eq(tb, attr_single(e1, "rejected"), "capsule cast"));
        CHECK(span_eq(tb, attr_single(e1, "reason"), "cheaper per frame"));
        CHECK(span_eq(tb, attr_single(e1, "about"), "player_jump"));
        CHECK(span_eq(tb, attr_single(e1, "resolves"), "ground check: raycast or capsule?"));
    }

    CHECK(tsub.fact_count == 2);
    {
        const SubFact *f0 = &tsub.facts[0], *f1 = &tsub.facts[1];
        CHECK(f0->is_triple);
        CHECK(span_eq(tb, f0->s, "player_jump"));
        CHECK(span_eq(tb, f0->p, "uses"));
        CHECK(span_eq(tb, f0->o, "coyote_time"));
        CHECK(f0->has_confidence && f0->confidence == 0.9);
        CHECK(f0->status == ST_ASSERTED && !f0->has_salience && span_absent(f0->src));

        CHECK(f1->is_triple && span_eq(tb, f1->o, "August 2026"));
        CHECK(f1->status == ST_DEFEASIBLE);
        CHECK(f1->has_confidence && f1->confidence == 0.6);
    }

    CHECK(tsub.change_count == 1);
    {
        const SubChange *c = &tsub.changes[0];
        CHECK(span_eq(tb, c->target, "jump_height"));
        CHECK(span_eq(tb, c->property, "value"));
        CHECK(span_eq(tb, c->to, "4.2"));
        CHECK(span_absent(c->from) && span_absent(c->event));
        CHECK(c->intervened == 1 && !c->has_confidence);
        CHECK(span_eq(tb, c->src, "src/player/jump.rs:142"));
    }

    CHECK(tsub.retract_count == 1);
    CHECK(tsub.retracts[0].is_rel_ref && span_eq(tb, tsub.retracts[0].rel_ref, "rel:502"));

    CHECK(tsub.focus_count == 1 && span_eq(tb, tsub.focus[0], "jump feel"));
    CHECK(tsub.observe == 0);
    CHECK(memcmp(tsub.intent, INTENT_DEFAULTS_SAVE, sizeof tsub.intent) == 0);
}

static void test_reader_accepts(void) {
    int failed;

    /* general-form fact, 2-5 slots, rel: alongside an element ref */
    TRY(parse_sub("{\"facts\":[{\"attrs\":{\"subject\":\"a\",\"with\":[\"b\",\"c\"],\"at\":\"rel:9\"}}]}"), failed);
    CHECK(!failed && tsub.fact_count == 1 && !tsub.facts[0].is_triple && tsub.facts[0].attr_count == 3);

    /* event-shaped general-form fact (from/to slots) parses as a plain fact */
    TRY(parse_sub("{\"facts\":[{\"attrs\":{\"target\":\"x\",\"from\":\"1\",\"to\":\"2\"}}]}"), failed);
    CHECK(!failed);

    /* fact-shape retract, both forms */
    TRY(parse_sub("{\"retract\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\"},{\"attrs\":{\"subject\":\"a\",\"uses\":\"b\"}}]}"), failed);
    CHECK(!failed && tsub.retract_count == 2);
    CHECK(!tsub.retracts[0].is_rel_ref && tsub.retracts[0].fact.is_triple);
    CHECK(!tsub.retracts[1].is_rel_ref && tsub.retracts[1].fact.attr_count == 2);

    /* templates + merge + intent + new; observe:false is accepted, true is not */
    TRY(parse_sub("{\"templates\":[{\"kind\":\"character\",\"expects\":[\"role\",\"wants\"],\"summary\":\"a person\"}],"
                  "\"merge\":[{\"from\":\"#118\",\"into\":\"#87\"}],"
                  "\"elements\":[{\"name\":\"Mercury\",\"new\":true}],"
                  "\"intent\":{\"arousal\":0.9},\"observe\":false}"), failed);
    CHECK(!failed);
    CHECK(tsub.template_count == 1 && span_eq(tb, tsub.templates[0].kind, "character"));
    CHECK(tsub.templates[0].expects_count == 2 &&
          span_eq(tb, tsub.span_pool[tsub.templates[0].expects_start + 1], "wants"));
    CHECK(tsub.merge_count == 1 && span_eq(tb, tsub.merges[0].from, "#118") &&
          span_eq(tb, tsub.merges[0].into, "#87"));
    CHECK(tsub.elements[0].is_new == 1);
    CHECK(tsub.observe == 0);
    CHECK(tsub.intent[INTENT_AROUSAL] == 0.9);
    CHECK(tsub.intent[INTENT_CONVICTION] == INTENT_DEFAULTS_SAVE[INTENT_CONVICTION]);
    /* a save that writes is not an observation (pin 28) */
    expect_sub_err("{\"observe\":true,\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\"}]}",
                   ERR_PARSE, "observe");

    /* change with event + from + confidence */
    TRY(parse_sub("{\"changes\":[{\"target\":\"t\",\"property\":\"p\",\"from\":\"1\",\"to\":\"2\","
                  "\"event\":\"the patch\",\"confidence\":0.5}]}"), failed);
    CHECK(!failed && span_eq(tb, tsub.changes[0].event, "the patch") &&
          tsub.changes[0].has_confidence && tsub.changes[0].confidence == 0.5);
}

static void test_reader_rejections(void) {
    /* unknown fields at any level, with a payload-path at */
    expect_sub_err("{\"focuss\":[\"x\"],\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\"}]}",
                   ERR_PARSE, "focuss");
    expect_sub_err("{\"elements\":[{\"name\":\"a\",\"colour\":\"red\"}]}",
                   ERR_PARSE, "elements[0].colour");
    expect_sub_err("{\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\",\"weight\":1}]}",
                   ERR_PARSE, "facts[0].weight");
    expect_sub_err("{\"intent\":{\"vibes\":0.5},\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\"}]}",
                   ERR_PARSE, "intent.vibes");
    /* a retract's fact shape carries no per-fact options */
    expect_sub_err("{\"retract\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\",\"status\":\"asserted\"}]}",
                   ERR_PARSE, "retract[0].status");

    /* wrong types */
    expect_sub_err("{\"facts\":\"nope\"}", ERR_PARSE, "facts");
    expect_sub_err("{\"elements\":[{\"name\":7}]}", ERR_PARSE, "elements[0].name");
    expect_sub_err("{\"observe\":\"yes\",\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\"}]}",
                   ERR_PARSE, "observe");
    expect_sub_err("{\"elements\":[{\"name\":\"a\",\"salience\":1.5}]}",
                   ERR_PARSE, "elements[0].salience");
    expect_sub_err("{\"elements\":[{\"name\":\"a\",\"attrs\":{\"k\":7}}]}",
                   ERR_PARSE, "elements[0].attrs.k");
    expect_sub_err("{\"elements\":[{\"name\":\"a\",\"attrs\":{\"k\":[]}}]}",
                   ERR_PARSE, "elements[0].attrs.k");

    /* missing / malformed required parts */
    expect_sub_err("{\"elements\":[{\"kind\":\"x\"}]}", ERR_PARSE, "elements[0]");
    expect_sub_err("{\"elements\":[{\"name\":\"\"}]}", ERR_PARSE, "elements[0].name");
    expect_sub_err("{\"facts\":[{\"s\":\"a\",\"p\":\"b\"}]}", ERR_PARSE, "facts[0]");
    expect_sub_err("{\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\",\"attrs\":{\"x\":\"y\"}}]}",
                   ERR_PARSE, "facts[0]");
    expect_sub_err("{\"facts\":[{\"attrs\":{\"only\":\"one\"}}]}", ERR_PARSE, "facts[0]");
    expect_sub_err("{\"facts\":[{\"attrs\":{\"a\":\"1\",\"b\":\"2\",\"c\":\"3\",\"d\":\"4\",\"e\":\"5\",\"f\":\"6\"}}]}",
                   ERR_PARSE, "facts[0]");
    expect_sub_err("{\"facts\":[{\"s\":\"rel:1\",\"p\":\"uses\",\"o\":\"rel:2\"}]}",
                   ERR_PARSE, "facts[0]");
    expect_sub_err("{\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\",\"status\":\"superseded\"}]}",
                   ERR_PARSE, "facts[0].status");
    expect_sub_err("{\"changes\":[{\"target\":\"a\",\"property\":\"p\"}]}", ERR_PARSE, "changes[0]");
    expect_sub_err("{\"templates\":[{\"expects\":[\"a\"]}]}", ERR_PARSE, "templates[0]");
    expect_sub_err("{\"merge\":[{\"from\":\"#1\"}]}", ERR_PARSE, "merge[0]");
    expect_sub_err("{\"retract\":[\"#5\"]}", ERR_PARSE, "retract[0]");
    expect_sub_err("{\"retract\":[\"rel:5x\"]}", ERR_PARSE, "retract[0]");
    expect_sub_err("{\"retract\":[7]}", ERR_PARSE, "retract[0]");

    /* a save needs at least one non-empty write list */
    expect_sub_err("{\"focus\":[\"x\"]}", ERR_PARSE, NULL);
    expect_sub_err("{\"facts\":[]}", ERR_PARSE, NULL);
    expect_sub_err("{}", ERR_PARSE, NULL);

    /* payload root must be an object */
    expect_sub_err("[1,2]", ERR_PARSE, NULL);

    /* > 64 entries in a list is limit_exceeded */
    {
        static char big[8192];
        u32 pos = 0, i;
        pos += (u32)snprintf(big + pos, sizeof big - pos, "{\"facts\":[");
        for (i = 0; i < 65; i++)
            pos += (u32)snprintf(big + pos, sizeof big - pos,
                                 "%s{\"s\":\"a\",\"p\":\"b\",\"o\":\"c%u\"}", i ? "," : "", i);
        snprintf(big + pos, sizeof big - pos, "]}");
        expect_sub_err(big, ERR_LIMIT_EXCEEDED, "facts");
    }
}

static void test_reader_recall(void) {
    int failed;

    TRY(parse_rec("{}"), failed); /* orientation mode: valid, all defaults */
    CHECK(!failed);
    CHECK(trec.focus_count == 0 && trec.limit == 40 && trec.history_depth == 2);
    CHECK(trec.since == -1 && trec.observe == 0 && trec.query.len == 0);

    TRY(parse_rec("{\"focus\":[\"jump physics\",\"#87\"],\"limit\":25,"
                  "\"history_depth\":3,\"since\":\"2026-06-01\",\"observe\":true}"), failed);
    CHECK(!failed);
    CHECK(trec.focus_count == 2);
    CHECK(span_eq(tb, trec.focus[0], "jump physics") && span_eq(tb, trec.focus[1], "#87"));
    CHECK(trec.limit == 25 && trec.history_depth == 3 && trec.observe == 1);
    CHECK(trec.since == 1780272000); /* 2026-06-01T00:00:00Z */

    TRY(parse_rec("{\"limit\":null,\"history_depth\":null}"), failed);
    CHECK(!failed && trec.limit == -1 && trec.history_depth == -1);

    TRY(parse_rec("{\"since\":\"2024-02-29\"}"), failed); /* leap day is valid */
    CHECK(!failed && trec.since == 1709164800);

    /* `query` (optional): the F1 ranking signal; accepted, never resolved */
    TRY(parse_rec("{\"query\":\"date of birth\"}"), failed);
    CHECK(!failed && trec.focus_count == 0 && span_eq(tb, trec.query, "date of birth"));
    expect_rec_err("{\"bogus\":\"x\"}", ERR_PARSE, "bogus"); /* unknown field still rejects */
    /* a recall is a submission with no writes (spec §4): intent is accepted */
    TRY(parse_rec("{\"intent\":{\"curiosity\":1}}"), failed);
    CHECK(!failed && trec.intent[INTENT_CURIOSITY] == 1.0);
    CHECK(trec.intent[INTENT_CONVICTION] == INTENT_DEFAULTS_RECALL[INTENT_CONVICTION]);
    TRY(parse_rec("{\"focus\":[\"x\"]}"), failed);
    CHECK(!failed && trec.intent[INTENT_CURIOSITY] == INTENT_DEFAULTS_RECALL[INTENT_CURIOSITY]);
    expect_rec_err("{\"intent\":{\"vibes\":1}}", ERR_PARSE, "intent.vibes");
    expect_rec_err("{\"limit\":0}", ERR_PARSE, "limit");
    expect_rec_err("{\"limit\":-3}", ERR_PARSE, "limit");
    expect_rec_err("{\"limit\":2.5}", ERR_PARSE, "limit");
    /* astronomically large limit = "everything": clamps to uncapped, no u32 wrap */
    TRY(parse_rec("{\"limit\":9000000000000000000}"), failed);
    CHECK(!failed && trec.limit == -1);
    expect_rec_err("{\"history_depth\":-1}", ERR_PARSE, "history_depth");
    expect_rec_err("{\"since\":\"2026-6-1\"}", ERR_PARSE, "since");
    expect_rec_err("{\"since\":\"2026-13-01\"}", ERR_PARSE, "since");
    expect_rec_err("{\"since\":\"2026-02-30\"}", ERR_PARSE, "since");
    expect_rec_err("{\"since\":\"2023-02-29\"}", ERR_PARSE, "since");
    expect_rec_err("{\"since\":20260601}", ERR_PARSE, "since");
    expect_rec_err("{\"focus\":\"jump\"}", ERR_PARSE, "focus");
    expect_rec_err("{\"focus\":[\"\"]}", ERR_PARSE, "focus[0]");
}

/* ------------------------------ CLI tests ----------------------------- */

static void test_payload_cap(void) {
    FILE *f = tmpfile();
    u32 i, n;
    CHECK(f != NULL);
    if (!f) return;
    for (i = 0; i < 70000; i++) fputc('x', f);
    rewind(f);
    n = read_payload_stream(f, tb, LEGEND_PAYLOAD_CAP + 1);
    CHECK(n == LEGEND_PAYLOAD_CAP + 1); /* byte 64Ki+1 arrived: caller raises limit_exceeded */
    fclose(f);

    f = tmpfile();
    CHECK(f != NULL);
    if (!f) return;
    for (i = 0; i < LEGEND_PAYLOAD_CAP; i++) fputc('x', f);
    rewind(f);
    n = read_payload_stream(f, tb, LEGEND_PAYLOAD_CAP + 1);
    CHECK(n == LEGEND_PAYLOAD_CAP); /* exactly at the cap is fine */
    fclose(f);
}

/* The journal tail carries the build of the last writer; graph_sync compares it
 * to ours on a reload it did not cause, which is how a server left behind by a
 * re-pin gets surfaced instead of writing unnoticed for days. */
static void test_journal_last_build(void) {
    char root_tmpl[] = "/tmp/legend_wp0_jb_XXXXXX";
    char *root = mkdtemp(root_tmpl);
    char p[4400], got[24];
    FILE *f;
    CHECK(root != NULL);
    if (!root) return;

    /* no journal at all */
    journal_last_build(root, got, sizeof got);
    CHECK(got[0] == '\0');

    snprintf(p, sizeof p, "%s/journal.jsonl", root);
    f = fopen(p, "wb");
    CHECK(f != NULL);
    if (!f) return;
    fputs("{\"ts\":1,\"build\":\"aaaaaaa\",\"verb\":\"save\"}\n", f);
    fputs("{\"ts\":2,\"build\":\"bbbbbbb\",\"verb\":\"recall\"}\n", f);
    fclose(f);
    /* the LAST line wins, not the first */
    journal_last_build(root, got, sizeof got);
    CHECK(strcmp(got, "bbbbbbb") == 0);

    /* a journal longer than the tail window still reads its final line */
    f = fopen(p, "wb");
    CHECK(f != NULL);
    if (!f) return;
    {
        int i;
        for (i = 0; i < 400; i++)
            fprintf(f, "{\"ts\":%d,\"build\":\"aaaaaaa\",\"verb\":\"save\","
                       "\"payload\":\"%0200d\"}\n", i, i);
        fputs("{\"ts\":999,\"build\":\"ccccccc\",\"verb\":\"save\"}\n", f);
    }
    fclose(f);
    journal_last_build(root, got, sizeof got);
    CHECK(strcmp(got, "ccccccc") == 0);
    unlink(p);
    rmdir(root);
}

static void test_store_discovery(void) {
    char root_tmpl[] = "/tmp/legend_wp0_store_XXXXXX";
    char *root = mkdtemp(root_tmpl);
    char saved_cwd[4096], p[4400], want[4400], out[4300];
    CHECK(root != NULL);
    if (!root) return;
    CHECK(getcwd(saved_cwd, sizeof saved_cwd) != NULL);

    snprintf(p, sizeof p, "%s/proj", root);            CHECK(mkdir(p, 0777) == 0);
    snprintf(p, sizeof p, "%s/proj/.legend", root);    CHECK(mkdir(p, 0777) == 0);
    snprintf(p, sizeof p, "%s/proj/a", root);          CHECK(mkdir(p, 0777) == 0);
    snprintf(p, sizeof p, "%s/proj/a/b", root);        CHECK(mkdir(p, 0777) == 0);

    /* walk-up finds the nearest .legend above the cwd */
    unsetenv("LEGEND_STATE_DIR");
    snprintf(p, sizeof p, "%s/proj/a/b", root);
    CHECK(chdir(p) == 0);
    snprintf(want, sizeof want, "%s/proj/.legend", root);
    CHECK(discover_store(out, sizeof out) == 1);
    CHECK(strcmp(out, want) == 0);

    /* LEGEND_STATE_DIR overrides the walk */
    setenv("LEGEND_STATE_DIR", root, 1);
    CHECK(discover_store(out, sizeof out) == 1);
    CHECK(strcmp(out, root) == 0);

    /* env set but missing: no store (only init creates) */
    snprintf(p, sizeof p, "%s/nowhere", root);
    setenv("LEGEND_STATE_DIR", p, 1);
    CHECK(discover_store(out, sizeof out) == 0);

    unsetenv("LEGEND_STATE_DIR");
    CHECK(chdir(saved_cwd) == 0);
}

static void test_flock_conflict(void) {
    char tmpl[] = "/tmp/legend_wp0_lock_XXXXXX";
    char *dir = mkdtemp(tmpl);
    char lockp[4400];
    PlatLock a, b;
    CHECK(dir != NULL);
    if (!dir) return;
    snprintf(lockp, sizeof lockp, "%s/legend.lock", dir);

    /* Two independent opens of one lock file must conflict INTRA-process. This
     * is the property plan §3.12 chose flock over fcntl record locks to get,
     * and it is the executable spec for the lock: fcntl would release on any
     * fd close and never conflict within a process, which would make the lock
     * silently useless AND untestable.
     *
     * Written through the seam rather than against flock(2) directly, so it
     * checks whichever implementation is compiled. The Win32 LockFileEx port
     * preserves both properties natively (locks are per-HANDLE), but it reports
     * contention as ERROR_LOCK_VIOLATION rather than EWOULDBLOCK -- so the test
     * asserts the seam's own three-way result (0 acquired / 1 busy / -1 error)
     * instead of an errno that does not exist on the other platform. Asserting
     * EWOULDBLOCK here is precisely how a wrong lock port would slip through. */
    CHECK(plat_lock_open(&a, lockp) >= 0);
    CHECK(plat_lock_try(&a) == 0);
    CHECK(plat_lock_open(&b, lockp) >= 0);
    CHECK(plat_lock_try(&b) == 1); /* busy, and NOT a hard error */
    plat_lock_close(&a);           /* release is by close, on both platforms */
    CHECK(plat_lock_try(&b) == 0);
    plat_lock_close(&b);

    /* acquire_lock() holds that same exclusive lock, and release_lock() drops
     * it -- the path mcp-serve uses per tools/call so it coexists with a
     * SessionStart hook running in another process. */
    acquire_lock(dir);
    CHECK(g_lock.fd >= 0);
    CHECK(plat_lock_open(&a, lockp) >= 0);
    CHECK(plat_lock_try(&a) == 1);
    release_lock();
    CHECK(plat_lock_try(&a) == 0);
    plat_lock_close(&a);
}

/* --------------------------- S3-S9 (M1) tests -------------------------- */

/* Shared substrate fixtures: static so trapped fail() longjmps leave state
 * reachable (LeakSanitizer-clean), same pattern as the reader fixtures. */
static Hypergraph tg, tg2;
static WriteReport twr;
static ByteBuf tbb1, tbb2;

static void fresh_graph(Hypergraph *g) {
    graph_free(g);
    ontology_seed(g);
    seed_ext_vocab(g); /* mirror cmd_init: extended vocab is part of a live store */
    seed_self(g);      /* ...as is the self anchor */
}

static void run_save_on(Hypergraph *g, const char *payload) {
    Rd r;
    size_t n = strlen(payload);
    memcpy(tb, payload, n + 1);
    json_parse(&tj, tb, (u32)n);
    r.t = tj.toks;
    r.buf = tb;
    read_submission(&r, &tsub);
    tick_save(g, &tsub, tb, &twr);
}

static void run_save(const char *payload) { run_save_on(&tg, payload); }

static u32 elem_by_name(const Hypergraph *g, const char *name) {
    u32 sid = str_find(&g->strs, name, (u32)strlen(name));
    volatile u32 i; /* callers inline this next to setjmp: keep -Wclobbered quiet */
    if (sid == NONE_U32) return NONE_U32;
    for (i = 0; i < g->element_count; i++)
        if (g->elements[i].names.v[0] == sid) return i;
    return NONE_U32;
}

static int elem_name_is(const Hypergraph *g, u32 id, const char *want) {
    u32 sid;
    if (id >= g->element_count) return 0;
    sid = g->elements[id].names.v[0];
    return str_len(&g->strs, sid) == strlen(want) &&
           memcmp(str_ptr(&g->strs, sid), want, str_len(&g->strs, sid)) == 0;
}

static void expect_save_err(const char *payload, int want_code, const char *want_at) {
    int failed;
    TRY(run_save(payload), failed);
    CHECK(failed);
    if (!failed) return;
    t_checks++;
    if (g_err.code != want_code) {
        t_fails++;
        printf("FAIL code %s want %s (payload %.60s)\n",
               ERR_CODE_NAMES[g_err.code], ERR_CODE_NAMES[want_code], payload);
    }
    if (want_at) {
        t_checks++;
        if (strcmp(g_err.at, want_at) != 0) {
            t_fails++;
            printf("FAIL at \"%s\" want \"%s\" (payload %.60s)\n", g_err.at, want_at, payload);
        }
    }
}

static void expect_fmt(double x, const char *want) {
    char out[8];
    u32 n = fmt_unit_float(x, out);
    t_checks++;
    if (strcmp(out, want) != 0 || n != strlen(want)) {
        t_fails++;
        printf("FAIL fmt_unit_float(%g) -> \"%s\", want \"%s\"\n", x, out, want);
    }
}

static void test_number_formatter(void) {
    /* the plan §3.7 table */
    expect_fmt(0.0, "0");
    expect_fmt(1.0, "1");
    expect_fmt(0.9, "0.9");
    expect_fmt(0.83, "0.83");
    expect_fmt(0.05, "0.05");
    expect_fmt(0.7, "0.7");
    /* rounding at the quantization edges */
    expect_fmt(0.005, "0.01");
    expect_fmt(0.004, "0");
    expect_fmt(0.999, "1");
    expect_fmt(0.71, "0.71");
    expect_fmt(0.1, "0.1");
    expect_fmt(0.55, "0.55");
    /* M5 fuzz regression: totality outside [0,1]. A snapshot only proves its
     * stats finite, and the old double->u32 cast of a huge or negative value
     * was undefined (gcc quietly printed "0" for 1e300). Clamp instead. */
    expect_fmt(1e300, "1");
    expect_fmt(2.0, "1");
    expect_fmt(-3.0, "0");
    expect_fmt(-1e300, "0");
    expect_fmt(1e-300, "0");
}

static void test_iso_format(void) {
    char out[40];
    format_iso_utc(1780272000, out);
    CHECK(strcmp(out, "2026-06-01T00:00:00Z") == 0);
    format_iso_utc(1780272000 + 3661, out);
    CHECK(strcmp(out, "2026-06-01T01:01:01Z") == 0);
    format_iso_utc(0, out);
    CHECK(strcmp(out, "1970-01-01T00:00:00Z") == 0);
    format_iso_utc(-1, out);
    CHECK(strcmp(out, "1969-12-31T23:59:59Z") == 0);
    format_iso_utc(1709164800, out); /* leap day, inverse of the reader test */
    CHECK(strcmp(out, "2024-02-29T00:00:00Z") == 0);
}

static void test_ontology_ids(void) {
    u32 i;
    fresh_graph(&tg);
    CHECK(tg.element_count == WK_ELEMENT_COUNT + EXT_COUNT + 1); /* 32 core + 10 ext + self */
    CHECK(tg.relation_count == 10);
    CHECK(tg.clock == 0);
    for (i = 0; i < WK_ELEMENT_COUNT; i++)
        CHECK(elem_name_is(&tg, i, WK_NAMES[i]));
    CHECK(elem_name_is(&tg, WK_SUBJECT, "subject"));
    CHECK(elem_name_is(&tg, WK_KIND_DECISION, "decision"));
    CHECK(elem_name_is(&tg, WK_KIND_COMMIT, "commit"));
    /* expects edges: rel:0 = {subject: decision, expects: chose} .. rel:9 */
    CHECK(tg.relations[0].attr_count == 2);
    CHECK(tg.relations[0].attrs[0].name == WK_SUBJECT &&
          tg.relations[0].attrs[0].value.id == WK_KIND_DECISION);
    CHECK(tg.relations[0].attrs[1].name == WK_EXPECTS &&
          tg.relations[0].attrs[1].value.id == WK_CHOSE);
    CHECK(tg.relations[4].attrs[1].value.id == WK_RESOLVES);
    CHECK(tg.relations[5].attrs[0].value.id == WK_KIND_CONSTRAINT);
    CHECK(tg.relations[8].attrs[0].value.id == WK_KIND_QUESTION &&
          tg.relations[8].attrs[1].value.id == WK_ABOUT);
    CHECK(tg.relations[9].attrs[0].value.id == WK_KIND_TASK);
    /* every kind carries a summary; the vocabulary does not */
    CHECK(tg.elements[WK_KIND_DECISION].summary != NONE_U32);
    CHECK(tg.elements[WK_SUBJECT].summary == NONE_U32);
    /* id stability: a second seed mints byte-identical state */
    fresh_graph(&tg2);
    snapshot_serialize(&tg, &tbb1);
    snapshot_serialize(&tg2, &tbb2);
    CHECK(tbb1.len == tbb2.len && memcmp(tbb1.v, tbb2.v, tbb1.len) == 0);
}

/* The self anchor: one seeded referent for first person, so `me` in a fact is
 * always the agent that wrote it (see SELF_NAME). */
static void test_self_anchor(void) {
    char tmpl[] = "/tmp/legend_self_XXXXXX";
    char *dir = mkdtemp(tmpl);
    u32 self, a, b;
    int failed;
    CHECK(dir != NULL);
    if (!dir) return;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    self = tg.wk_self;
    CHECK(self != NONE_U32 && elem_name_is(&tg, self, "me"));
    /* vocabulary, not content: salience 0 keeps it out of the embed list and
     * the orientation packet, and it carries no kind */
    CHECK(tg.elements[self].stats.salience == 0.0);
    CHECK(tg.elem_kind[self] == NONE_U32);
    CHECK(tg.elements[self].summary != NONE_U32);

    /* every alias resolves to the ONE self -- a session reaching for a
     * different surface form must not mint a second agent */
    TRY(run_save("{\"facts\":[{\"s\":\"the assistant\",\"p\":\"removes\","
                 "\"o\":\"em dash\"}]}"), failed);
    CHECK(!failed);
    CHECK(twr.reused_elems.count == 1 && twr.reused_elems.v[0] == self);
    TRY(run_save("{\"facts\":[{\"s\":\"myself\",\"p\":\"removes\",\"o\":\"em dash\"}]}"),
        failed);
    CHECK(!failed && twr.minted_elems.count == 0);
    TRY(run_save("{\"facts\":[{\"s\":\"ME\",\"p\":\"removes\",\"o\":\"em dash\"}]}"),
        failed);
    CHECK(!failed && twr.minted_elems.count == 0);

    /* the qualified form: {s,p,o} plus a qualifier on the statement */
    TRY(run_save("{\"elements\":[{\"name\":\"Nick\",\"kind\":\"person\"}],"
                 "\"facts\":[{\"s\":\"Nick\",\"p\":\"asked\",\"o\":\"me\"},"
                 "{\"attrs\":{\"subject\":\"me\",\"removes\":\"em dash\","
                 "\"from\":\"text\"}}]}"),
        failed);
    CHECK(!failed);
    a = tg.wk_self;
    CHECK(a == self); /* the anchor never moves */

    /* protected: renaming or merging it away would orphan every claim on it */
    TRY(run_save("{\"elements\":[{\"name\":\"me\",\"rename_to\":\"the bot\"}]}"), failed);
    CHECK(failed);
    TRY(run_save("{\"merge\":[{\"from\":\"me\",\"into\":\"Nick\"}]}"), failed);
    CHECK(failed);

    /* re-opening a store that already carries the anchor adopts it rather than
     * minting a second one */
    b = tg.element_count;
    snapshot_write(&tg, dir);
    graph_free(&tg2);
    CHECK(snapshot_load(&tg2, dir) == 1);
    CHECK(tg2.element_count == b && tg2.wk_self == self);
    CHECK(elem_name_is(&tg2, tg2.wk_self, "me"));

    /* and a store written BEFORE the anchor existed gains it on open, with no
     * migration: exactly one new element, no relation churn (the seed_ext_vocab
     * contract -- this is the path every live store takes at the upgrade) */
    graph_free(&tg);
    ontology_seed(&tg);
    seed_ext_vocab(&tg); /* deliberately no seed_self: a pre-anchor store */
    b = tg.element_count;
    a = tg.relation_count;
    snapshot_write(&tg, dir);
    graph_free(&tg2);
    CHECK(snapshot_load(&tg2, dir) == 1);
    CHECK(tg2.element_count == b + 1);
    CHECK(tg2.relation_count == a);
    CHECK(tg2.wk_self == b && elem_name_is(&tg2, tg2.wk_self, "me"));
}

static void test_tier1_resolution(void) {
    u32 planet, mercury_a, mercury_b;
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"elements\":[{\"name\":\"Mercury\",\"kind\":\"planet\"}]}"), failed);
    CHECK(!failed && twr.minted_elems.count == 2);
    mercury_a = twr.minted_elems.v[0];
    planet = twr.minted_elems.v[1];
    CHECK(elem_name_is(&tg, mercury_a, "Mercury") && elem_name_is(&tg, planet, "planet"));
    CHECK(tg.elem_kind[mercury_a] == planet);

    /* tier-1 is normalized-exact: surface variants resolve, not re-mint */
    TRY(run_save("{\"facts\":[{\"s\":\"MERCURY\",\"p\":\"orbits\",\"o\":\"the sun\"}]}"), failed);
    CHECK(!failed);
    CHECK(twr.reused_elems.count == 1 && twr.reused_elems.v[0] == mercury_a);

    /* new: true forces a homonym mint (person is the seeded kind #28) */
    TRY(run_save("{\"elements\":[{\"name\":\"Mercury\",\"kind\":\"person\",\"new\":true}]}"), failed);
    CHECK(!failed && twr.minted_elems.count == 1);
    CHECK(twr.reused_elems.count == 1 && twr.reused_elems.v[0] == WK_KIND_PERSON);
    mercury_b = twr.minted_elems.v[0];
    CHECK(mercury_b != mercury_a && elem_name_is(&tg, mercury_b, "Mercury"));

    /* bare homonym ref: no kind context -> highest last_seen wins (B, newer) */
    TRY(run_save("{\"facts\":[{\"s\":\"mercury\",\"p\":\"likes\",\"o\":\"tea\"}]}"), failed);
    CHECK(!failed);
    CHECK(twr.reused_elems.count == 1 && twr.reused_elems.v[0] == mercury_b);

    /* kind context: prefer the matching kind over recency */
    TRY(run_save("{\"elements\":[{\"name\":\"Mercury\",\"kind\":\"planet\"}]}"), failed);
    CHECK(!failed && twr.minted_elems.count == 0);
    CHECK(twr.reused_elems.count == 2);
    CHECK(twr.reused_elems.v[0] == mercury_a); /* name first, then kind (plan §3.17) */
    CHECK(twr.reused_elems.v[1] == planet);

    /* #id refs resolve exactly; unknown ids are hard errors */
    TRY(run_save("{\"facts\":[{\"s\":\"#0\",\"p\":\"#1\",\"o\":\"#2\"}]}"), failed);
    CHECK(!failed);
    expect_save_err("{\"facts\":[{\"s\":\"#9999\",\"p\":\"a\",\"o\":\"b\"}]}",
                    ERR_UNKNOWN_REF, "facts[0].s");
    expect_save_err("{\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"rel:9999\"}]}",
                    ERR_UNKNOWN_REF, "facts[0].o");
    expect_save_err("{\"focus\":[\"#9999\"],\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\"}]}",
                    ERR_UNKNOWN_REF, "focus[0]");

    /* empty-after-normalize is a parse error at the ref path (S5) */
    expect_save_err("{\"facts\":[{\"s\":\"---\",\"p\":\"b\",\"o\":\"c\"}]}",
                    ERR_PARSE, "facts[0].s");
    expect_save_err("{\"elements\":[{\"name\":\"?!\"}]}", ERR_PARSE, "elements[0].name");
    expect_save_err("{\"elements\":[{\"name\":\"a\",\"aliases\":[\"..\"]}]}",
                    ERR_PARSE, "elements[0].aliases[0]");
    expect_save_err("{\"focus\":[\"..\"],\"facts\":[{\"s\":\"a\",\"p\":\"b\",\"o\":\"c\"}]}",
                    ERR_PARSE, "focus[0]");

    /* an event-shaped fact without a property slot cannot key a cache */
    expect_save_err("{\"facts\":[{\"attrs\":{\"target\":\"x\",\"from\":\"1\",\"to\":\"2\"}}]}",
                    ERR_PARSE, "facts[0]");
    unsetenv("LEGEND_NOW");
}

/* the "immutable kind" trial fix: resubmitting an element with a different kind
 * supersedes the old instance_of and moves elem_kind to the new kind (even when
 * the new kind is minted this tick); resubmitting the same kind is a no-op. */
static void test_kind_change(void) {
    u32 foo, decision_k, module_k, rid, a, live, live_kind;
    int failed;
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"foo\",\"kind\":\"decision\",\"summary\":\"x\"}]}"), failed);
    CHECK(!failed);
    foo = elem_by_name(&tg, "foo");
    decision_k = elem_by_name(&tg, "decision");
    CHECK(foo != NONE_U32 && tg.elem_kind[foo] == decision_k);

    /* correct the kind: decision -> module (module is minted this tick) */
    TRY(run_save("{\"elements\":[{\"name\":\"foo\",\"kind\":\"module\",\"summary\":\"x\"}]}"), failed);
    CHECK(!failed);
    module_k = elem_by_name(&tg, "module");
    CHECK(module_k != NONE_U32 && tg.elem_kind[foo] == module_k);

    /* exactly one LIVE instance_of on foo, pointing at module (old superseded) */
    live = 0;
    live_kind = NONE_U32;
    for (rid = 0; rid < tg.relation_count; rid++) {
        const Relation *r = &tg.relations[rid];
        int subj = 0;
        u32 kv = NONE_U32;
        if (r->status >= ST_SUPERSEDED || r->attr_count != 2)
            continue;
        for (a = 0; a < 2; a++) {
            if (r->attrs[a].name == WK_SUBJECT && r->attrs[a].value.tag == TERM_ELEM &&
                r->attrs[a].value.id == foo)
                subj = 1;
            if (r->attrs[a].name == WK_INSTANCE_OF && r->attrs[a].value.tag == TERM_ELEM)
                kv = r->attrs[a].value.id;
        }
        if (subj && kv != NONE_U32) {
            live++;
            live_kind = kv;
        }
    }
    CHECK(live == 1 && live_kind == module_k);

    /* resubmit the SAME kind -> idempotent, kind stays module */
    TRY(run_save("{\"elements\":[{\"name\":\"foo\",\"kind\":\"module\",\"summary\":\"x\"}]}"), failed);
    CHECK(!failed && tg.elem_kind[foo] == module_k);
}

static void test_relation_dedup(void) {
    int failed;
    u32 rel, before;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"source\":\"alice\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 1 && twr.reused_rel_count == 0);
    rel = twr.minted_rels.v[0];
    CHECK(tg.relations[rel].stats.support_count == 1);
    CHECK(tg.relations[rel].stats.support_diversity == 1);
    CHECK(tg.relations[rel].supporters.count == 1 &&
          elem_name_is(&tg, tg.relations[rel].supporters.v[0], "alice"));

    /* same source restating: support grows, diversity does not */
    TRY(run_save("{\"source\":\"alice\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 0 && twr.reused_rel_count == 1);
    CHECK(twr.reused_rels[0].rel == rel && twr.reused_rels[0].support_count == 2);
    CHECK(tg.relations[rel].stats.support_diversity == 1);

    /* a distinct source: diversity increments and the supporter is recorded */
    TRY(run_save("{\"source\":\"bob\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.reused_rel_count == 1 && twr.reused_rels[0].support_count == 3);
    CHECK(tg.relations[rel].stats.support_diversity == 2);
    CHECK(tg.relations[rel].supporters.count == 2 &&
          elem_name_is(&tg, tg.relations[rel].supporters.v[1], "bob"));

    /* attr-set equality ignores slot spelling: general form dedupes triple */
    TRY(run_save("{\"facts\":[{\"attrs\":{\"subject\":\"a\",\"uses\":\"b\"}}]}"), failed);
    CHECK(!failed && twr.reused_rel_count == 1 && twr.reused_rels[0].rel == rel);
    CHECK(twr.reused_rels[0].support_count == 4);

    /* a same-payload duplicate folds into one mint at support 2 */
    before = tg.relation_count;
    TRY(run_save("{\"facts\":[{\"s\":\"x\",\"p\":\"uses\",\"o\":\"y\"},"
                 "{\"s\":\"x\",\"p\":\"uses\",\"o\":\"y\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 1 && twr.reused_rel_count == 0);
    CHECK(tg.relations[twr.minted_rels.v[0]].stats.support_count == 2);
    CHECK(tg.relation_count == before + 1);

    /* a different status is still the same attr set: dedup, not a new mint */
    TRY(run_save("{\"facts\":[{\"s\":\"x\",\"p\":\"uses\",\"o\":\"y\",\"status\":\"defeasible\"}]}"), failed);
    CHECK(!failed && twr.reused_rel_count == 1);
    unsetenv("LEGEND_NOW");
}

static void test_write_report_shape(void) {
    int failed;
    u32 before_rels;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    before_rels = tg.relation_count;
    TRY(run_save("{\"source\":\"t\","
                 "\"elements\":[{\"name\":\"gc\",\"kind\":\"decision\","
                 "\"attrs\":{\"chose\":\"x\",\"about\":\"gc2\"}}],"
                 "\"facts\":[{\"s\":\"gc2\",\"p\":\"uses\",\"o\":\"x\",\"src\":\"f.rs:1\"}]}"), failed);
    CHECK(!failed);
    /* minted, first-touch order: gc, x, gc2, uses, f.rs:1, t (source last) */
    CHECK(twr.minted_elems.count == 6);
    CHECK(elem_name_is(&tg, twr.minted_elems.v[0], "gc"));
    CHECK(elem_name_is(&tg, twr.minted_elems.v[1], "x"));
    CHECK(elem_name_is(&tg, twr.minted_elems.v[2], "gc2"));
    CHECK(elem_name_is(&tg, twr.minted_elems.v[3], "uses"));
    CHECK(elem_name_is(&tg, twr.minted_elems.v[4], "f.rs:1"));
    CHECK(elem_name_is(&tg, twr.minted_elems.v[5], "t"));
    /* reused: only the kind; chose/about are vocabulary, t is the source */
    CHECK(twr.reused_elems.count == 1);
    CHECK(twr.reused_elems.v[0] == WK_KIND_DECISION);
    /* base relations contiguous in walk order: 2 attr rels + 1 fact */
    CHECK(twr.minted_rels.count == 3);
    CHECK(twr.minted_rels.v[0] == before_rels);
    CHECK(twr.minted_rels.v[1] == before_rels + 1);
    CHECK(twr.minted_rels.v[2] == before_rels + 2);
    /* metas follow: instance_of(gc), instance_of(pointer), src, source x3 */
    CHECK(tg.relation_count == before_rels + 3 + 2 + 1 + 3);
    /* the src pointer element was typed pointer */
    CHECK(tg.elem_kind[twr.minted_elems.v[4]] == WK_KIND_POINTER);
    CHECK(twr.tick == 1);
    CHECK(twr.at_secs == 1780272000);

    /* templates: kind + expects vocabulary mint and the expects edge lists */
    TRY(run_save("{\"templates\":[{\"kind\":\"character\",\"expects\":[\"role\",\"wants\"],"
                 "\"summary\":\"a person in the novel\"}]}"), failed);
    CHECK(!failed);
    CHECK(twr.minted_elems.count == 3); /* character, role, wants */
    CHECK(elem_name_is(&tg, twr.minted_elems.v[0], "character"));
    CHECK(twr.minted_rels.count == 2);
    CHECK(tg.elements[twr.minted_elems.v[0]].summary != NONE_U32);

    /* aliases + summary land on the element; alias joins the name index */
    TRY(run_save("{\"elements\":[{\"name\":\"jump_physics\",\"aliases\":[\"jump feel\"],"
                 "\"summary\":\"the jump system\",\"salience\":0.8}]}"), failed);
    CHECK(!failed);
    {
        volatile u32 jp = elem_by_name(&tg, "jump_physics"); /* lives across TRY */
        CHECK(jp != NONE_U32);
        CHECK(tg.elements[jp].names.count == 2);
        CHECK(tg.elements[jp].summary != NONE_U32);
        CHECK(tg.elements[jp].stats.salience == 0.8);
        /* the alias resolves */
        TRY(run_save("{\"facts\":[{\"s\":\"Jump-Feel\",\"p\":\"uses\",\"o\":\"zed\"}]}"), failed);
        CHECK(!failed && twr.reused_elems.count >= 1 && twr.reused_elems.v[0] == jp);
    }
    unsetenv("LEGEND_NOW");
}

static void test_store_full(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    g_cap_elements = tg.element_count; /* any element mint overflows */
    TRY(run_save("{\"facts\":[{\"s\":\"newthing\",\"p\":\"uses\",\"o\":\"subject\"}]}"), failed);
    CHECK(failed && g_err.code == ERR_STORE_FULL);
    g_cap_elements = 0xFFFFFFFEu;

    fresh_graph(&tg);
    g_cap_relations = tg.relation_count;
    TRY(run_save("{\"facts\":[{\"s\":\"subject\",\"p\":\"target\",\"o\":\"from\"}]}"), failed);
    CHECK(failed && g_err.code == ERR_STORE_FULL);
    g_cap_relations = 0xFFFFFFFEu;

    fresh_graph(&tg);
    g_cap_strings = tg.strs.count;
    TRY(run_save("{\"facts\":[{\"s\":\"brand new name\",\"p\":\"uses\",\"o\":\"subject\"}]}"), failed);
    CHECK(failed && g_err.code == ERR_STORE_FULL);
    g_cap_strings = 0xFFFFFFFEu;

    fresh_graph(&tg);
    g_cap_ticks = tg.clock; /* the next tick would overflow */
    TRY(run_save("{\"facts\":[{\"s\":\"subject\",\"p\":\"target\",\"o\":\"from\"}]}"), failed);
    CHECK(failed && g_err.code == ERR_STORE_FULL);
    CHECK(tg.clock == 0 && tg.relation_count == 10); /* guard fired before mutation */
    g_cap_ticks = 0xFFFFFFFEu;
    unsetenv("LEGEND_NOW");
}

static void write_raw_file(const char *path, const u8 *bytes, u32 len) {
    FILE *f = fopen(path, "wb");
    CHECK(f != NULL);
    if (!f) return;
    CHECK(fwrite(bytes, 1, len, f) == len);
    fclose(f);
}

static int file_bytes_eq(const char *a, const char *b) {
    static char ba[1 << 20], bb2[1 << 20];
    FILE *fa = fopen(a, "rb"), *fb = fopen(b, "rb");
    size_t na, nb;
    if (!fa || !fb) { if (fa) fclose(fa); if (fb) fclose(fb); return 0; }
    na = fread(ba, 1, sizeof ba, fa);
    nb = fread(bb2, 1, sizeof bb2, fb);
    fclose(fa);
    fclose(fb);
    return na == nb && memcmp(ba, bb2, na) == 0;
}

static void test_persistence_roundtrip(void) {
    char tmpl[] = "/tmp/legend_wp2_persist_XXXXXX";
    char *dir = mkdtemp(tmpl);
    char snap1[4400], snap2[4500], d2[4400];
    int failed;
    CHECK(dir != NULL);
    if (!dir) return;
    snprintf(snap1, sizeof snap1, "%s/legend.snapshot", dir);
    snprintf(d2, sizeof d2, "%s/two", dir);
    CHECK(mkdir(d2, 0777) == 0);
    snprintf(snap2, sizeof snap2, "%s/legend.snapshot", d2);

    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"source\":\"alice\",\"elements\":[{\"name\":\"coyote_time\","
                 "\"kind\":\"mechanic\",\"aliases\":[\"coyote frames\"],"
                 "\"summary\":\"grace window\",\"salience\":0.8}],"
                 "\"facts\":[{\"s\":\"player_jump\",\"p\":\"uses\",\"o\":\"coyote_time\","
                 "\"confidence\":0.9,\"status\":\"defeasible\",\"src\":\"src/j.rs:1\"}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1780358400", 1);
    TRY(run_save("{\"facts\":[{\"s\":\"player_jump\",\"p\":\"uses\",\"o\":\"coyote_time\","
                 "\"confidence\":0.9,\"status\":\"defeasible\"},"
                 "{\"attrs\":{\"subject\":\"beta\",\"at\":\"August 2026\",\"with\":\"rel:0\"}}]}"), failed);
    CHECK(!failed);

    /* stamp table round-trip material: two ticks, two wall stamps */
    CHECK(tg.clock == 2);
    CHECK(tg.stamps[0] == 1780272000 && tg.stamps[1] == 1780358400);

    snapshot_write(&tg, dir);

    /* save -> load -> save is byte-identical (M1 gate property) */
    graph_free(&tg2);
    CHECK(snapshot_load(&tg2, dir) == 1);
    CHECK(tg2.element_count == tg.element_count);
    CHECK(tg2.relation_count == tg.relation_count);
    CHECK(tg2.clock == 2 && tg2.stamps[0] == 1780272000 && tg2.stamps[1] == 1780358400);
    CHECK(tg2.strs.count == tg.strs.count);
    snapshot_serialize(&tg, &tbb1);
    snapshot_serialize(&tg2, &tbb2);
    CHECK(tbb1.len == tbb2.len && memcmp(tbb1.v, tbb2.v, tbb1.len) == 0);
    snapshot_write(&tg2, d2);
    CHECK(file_bytes_eq(snap1, snap2));

    /* the rebuilt indices behave: dedup still fires across the reload */
    setenv("LEGEND_NOW", "1780444800", 1);
    TRY(run_save_on(&tg2, "{\"facts\":[{\"s\":\"player_jump\",\"p\":\"uses\","
                          "\"o\":\"coyote_time\",\"confidence\":0.9,\"status\":\"defeasible\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 0 && twr.reused_rel_count == 1);
    CHECK(twr.reused_rels[0].support_count == 3);
    /* and the alias index too */
    TRY(run_save_on(&tg2, "{\"elements\":[{\"name\":\"coyote frames\"}]}"), failed);
    CHECK(!failed && twr.minted_elems.count == 0 && twr.reused_elems.count == 1);
    CHECK(elem_name_is(&tg2, twr.reused_elems.v[0], "coyote_time"));

    /* orphan-tmp sweep: eats snapshot tmps, never the lock */
    {
        char p[4500];
        struct stat st;
        snprintf(p, sizeof p, "%s/legend.snapshot.tmp", dir);
        write_raw_file(p, (const u8 *)"junk", 4);
        snprintf(p, sizeof p, "%s/legend.lock", dir);
        write_raw_file(p, (const u8 *)"", 0);
        sweep_orphan_tmps(dir);
        snprintf(p, sizeof p, "%s/legend.snapshot.tmp", dir);
        CHECK(stat(p, &st) != 0);
        snprintf(p, sizeof p, "%s/legend.lock", dir);
        CHECK(stat(p, &st) == 0);
        snprintf(p, sizeof p, "%s/legend.snapshot", dir);
        CHECK(stat(p, &st) == 0);
    }
    unsetenv("LEGEND_NOW");
}

/* ---- corrupt snapshots: every case must be snapshot_corrupt, never UB ---- */

static u32 rd32le(const u8 *p) {
    return (u32)p[0] | (u32)p[1] << 8 | (u32)p[2] << 16 | (u32)p[3] << 24;
}

static void wr32le(u8 *p, u32 v) {
    p[0] = (u8)v; p[1] = (u8)(v >> 8); p[2] = (u8)(v >> 16); p[3] = (u8)(v >> 24);
}

static void wr64le(u8 *p, u64 v) {
    int i;
    for (i = 0; i < 8; i++) p[i] = (u8)(v >> (8 * i));
}

enum { SNAP_STRINGS_OFF = 128 }; /* header 20 + clock 4 + policy 100 + stamp_count 4, clock 0 */

static u32 snap_elements_off(const u8 *b) {
    u32 off = SNAP_STRINGS_OFF;
    u32 n = rd32le(b + off), i;
    off += 4;
    for (i = 0; i < n; i++) off += 4 + rd32le(b + off);
    return off;
}

static u32 snap_relations_off(const u8 *b) {
    u32 off = snap_elements_off(b);
    u32 n = rd32le(b + off), i;
    off += 4;
    for (i = 0; i < n; i++) {
        u32 nc = rd32le(b + off);
        off += 4 + nc * 4 + 12 + 52; /* names, summary+redirect+created, stats */
    }
    return off;
}

static char t_corrupt_dir[4400];

static void expect_corrupt(const u8 *bytes, u32 len, const char *what) {
    char p[4500];
    int failed;
    snprintf(p, sizeof p, "%s/legend.snapshot", t_corrupt_dir);
    write_raw_file(p, bytes, len);
    graph_free(&tg2);
    g_err.code = -1;
    TRY((void)snapshot_load(&tg2, t_corrupt_dir), failed);
    t_checks++;
    if (!failed || g_err.code != ERR_SNAPSHOT_CORRUPT) {
        t_fails++;
        printf("FAIL corrupt case \"%s\": failed=%d code=%s\n", what, failed,
               failed ? ERR_CODE_NAMES[g_err.code] : "(none)");
    }
}

static void test_snapshot_corrupt(void) {
    static u8 mut[1 << 20];
    char tmpl[] = "/tmp/legend_wp2_corrupt_XXXXXX";
    char *dir = mkdtemp(tmpl);
    u32 len, n, eoff, roff;
    CHECK(dir != NULL);
    if (!dir) return;
    snprintf(t_corrupt_dir, sizeof t_corrupt_dir, "%s", dir);

    /* base: a fresh seeded store at clock 0 — deterministic offsets */
    fresh_graph(&tg);
    snapshot_serialize(&tg, &tbb1);
    len = tbb1.len;
    CHECK(len > 200 && len < sizeof mut);
    eoff = snap_elements_off(tbb1.v);
    roff = snap_relations_off(tbb1.v);
    CHECK(rd32le(tbb1.v + eoff) == 43 && rd32le(tbb1.v + roff) == 10);

    /* truncation at every prefix: declared-length mismatch, cleanly */
    for (n = 0; n < len; n += (n < 256 ? 1 : 17)) {
        memcpy(mut, tbb1.v, n);
        expect_corrupt(mut, n, "truncated");
    }
    /* truncation with the declared length patched to match: the deep checks
     * (counts, ranges, trailing) must all fail cleanly too */
    for (n = 20; n < len; n += (n < 256 ? 1 : 17)) {
        memcpy(mut, tbb1.v, n);
        wr64le(mut + 12, n);
        expect_corrupt(mut, n, "truncated+patched-length");
    }

    memcpy(mut, tbb1.v, len);
    mut[0] ^= 0xFF; /* bad magic */
    expect_corrupt(mut, len, "bad magic");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + 8, 2); /* unknown version */
    expect_corrupt(mut, len, "bad version");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + 20, 5); /* clock 5 but stamp_count 0 */
    expect_corrupt(mut, len, "stamp table mismatch");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + 124, 1); /* stamp_count 1 but clock 0 */
    expect_corrupt(mut, len, "stamp count mismatch");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + SNAP_STRINGS_OFF, 0xFFFFFF00u); /* oversized string count */
    expect_corrupt(mut, len, "oversized string count");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + SNAP_STRINGS_OFF + 4, 0x0FFFFFFFu); /* first string length */
    expect_corrupt(mut, len, "oversized string length");

    memcpy(mut, tbb1.v, len);
    mut[SNAP_STRINGS_OFF + 8] = 'x'; /* "subject" -> "xubject": ontology check */
    expect_corrupt(mut, len, "ontology name mismatch");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + eoff, 0xFFFFFF00u); /* oversized element count */
    expect_corrupt(mut, len, "oversized element count");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + eoff + 4, 0); /* first element name_count 0 */
    expect_corrupt(mut, len, "zero name count");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + eoff + 8, 0x7FFFFFFFu); /* first element name id out of range */
    expect_corrupt(mut, len, "name id out of range");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + eoff + 16, 500); /* redirect out of range (32 elements) */
    expect_corrupt(mut, len, "redirect out of range");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + eoff + 20, 99); /* created_at beyond clock 0 */
    expect_corrupt(mut, len, "created after clock");

    memcpy(mut, tbb1.v, len);
    wr64le(mut + eoff + 24, 0x7FF0000000000000u); /* confidence = +inf */
    expect_corrupt(mut, len, "non-finite stat");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + roff, 0xFFFFFF00u); /* oversized relation count */
    expect_corrupt(mut, len, "oversized relation count");

    memcpy(mut, tbb1.v, len);
    mut[roff + 4] = 7; /* attr_count 7 */
    expect_corrupt(mut, len, "bad attr count");

    memcpy(mut, tbb1.v, len);
    mut[roff + 5] = 9; /* status 9 */
    expect_corrupt(mut, len, "bad status");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + roff + 6, 0x7FFFFFFFu); /* slot name out of range */
    expect_corrupt(mut, len, "slot name out of range");

    memcpy(mut, tbb1.v, len);
    mut[roff + 10] = 3; /* term tag 3 */
    expect_corrupt(mut, len, "bad term tag");

    memcpy(mut, tbb1.v, len);
    wr32le(mut + roff + 11, 0x7FFFFFFFu); /* element term out of range */
    expect_corrupt(mut, len, "term id out of range");

    /* trailing garbage behind a matching declared length */
    memcpy(mut, tbb1.v, len);
    mut[len] = 0xAB;
    wr64le(mut + 12, len + 1);
    expect_corrupt(mut, len + 1, "trailing bytes");

    /* M5 fuzz regression (fuzz/fuzz_snapshot.py): a flipped byte inside a
     * stored non-ontology string used to load cleanly, and every frame that
     * echoed the name stopped being valid JSON text. The string table
     * validates as UTF-8 now. */
    {
        int failed;
        u32 off, hit = NONE_U32;
        setenv("LEGEND_NOW", "1780272000", 1);
        TRY(run_save_on(&tg, "{\"elements\":[{\"name\":\"caf\xC3\xA9 strudel\"}]}"), failed);
        unsetenv("LEGEND_NOW");
        CHECK(!failed);
        snapshot_serialize(&tg, &tbb2);
        CHECK(tbb2.len > len && tbb2.len < sizeof mut);
        for (off = 0; off + 13 <= tbb2.len; off++)
            if (memcmp(tbb2.v + off, "caf\xC3\xA9 strudel", 13) == 0) { hit = off; break; }
        CHECK(hit != NONE_U32);
        if (hit != NONE_U32) {
            memcpy(mut, tbb2.v, tbb2.len);
            mut[hit + 3] = 0xFF; /* the \xC3 lead byte */
            expect_corrupt(mut, tbb2.len, "string not UTF-8");
        }
    }

    /* and the pristine bytes still load (the harness isn't crying wolf) */
    {
        char p[4500];
        int failed;
        snprintf(p, sizeof p, "%s/legend.snapshot", t_corrupt_dir);
        write_raw_file(p, tbb1.v, len);
        graph_free(&tg2);
        TRY((void)snapshot_load(&tg2, t_corrupt_dir), failed);
        CHECK(!failed);
        CHECK(tg2.element_count == 43 && tg2.relation_count == 10);
    }
}

/* --------------------------- M2 write-path tests -------------------------- */

/* First relation whose attr set is {subject: subj, name: val} (both elements). */
static u32 rel_find_pair(const Hypergraph *g, u32 name_elem, u32 subj, u32 val) {
    Attr attrs[2];
    attrs[0].name = WK_SUBJECT;
    attrs[0].value.tag = TERM_ELEM;
    attrs[0].value.id = subj;
    attrs[1].name = name_elem;
    attrs[1].value.tag = TERM_ELEM;
    attrs[1].value.id = val;
    return dedup_lookup_live(g, attrs, 2);
}

/* A meta {subject: rel subj, <name>: rel obj} exists and is not retracted. */
static int has_rel_meta(const Hypergraph *g, u32 subj_rel, u32 name_elem, u32 obj_rel) {
    u32 i, a;
    for (i = 0; i < g->relation_count; i++) {
        const Relation *r = &g->relations[i];
        int subj_ok = 0, obj_ok = 0;
        if (r->status == ST_RETRACTED || r->attr_count != 2) continue;
        for (a = 0; a < 2; a++) {
            if (r->attrs[a].name == WK_SUBJECT && r->attrs[a].value.tag == TERM_REL &&
                r->attrs[a].value.id == subj_rel)
                subj_ok = 1;
            if (r->attrs[a].name == name_elem && r->attrs[a].value.tag == TERM_REL &&
                r->attrs[a].value.id == obj_rel)
                obj_ok = 1;
        }
        if (subj_ok && obj_ok) return 1;
    }
    return 0;
}

static void test_prose_backstop(void) {
    /* trial R5 backstop: a changes.to that would MINT a prose element (>120 norm
     * chars) is rejected at changes[i].to; short values and long values that RESOLVE
     * to an existing element (references, not new mints) pass. */
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    /* prose value that would mint -> rejected */
    expect_save_err(
        "{\"changes\":[{\"target\":\"build\",\"property\":\"build_status\",\"to\":"
        "\"BUILT 2026-07-21 sim-side, ART IS PLACEHOLDER (she wears older frames in "
        "the viewer); the real 8-anim set is next and blocks the ship\"}]}",
        ERR_PROSE_VALUE, "changes[0].to");
    /* short canonical value passes */
    fresh_graph(&tg);
    TRY(run_save("{\"changes\":[{\"target\":\"build\",\"property\":\"build_status\","
                 "\"to\":\"built\"}]}"), failed);
    CHECK(!failed);
    /* entity-model name gate: a fresh element name over 5 words reads as a claim,
     * not an entity -> rejected at elements[i].name */
    fresh_graph(&tg);
    expect_save_err(
        "{\"elements\":[{\"name\":\"input latency stays under one frame\","
        "\"kind\":\"constraint\"}]}",
        ERR_PROSE_VALUE, "elements[0].name");
    /* a short noun handle (<=5 words) passes */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"input latency budget\","
                 "\"kind\":\"constraint\"}]}"), failed);
    CHECK(!failed);
    /* kind gate: a multi-word kind is a claim, and accepting one supersedes the
     * element's real kind -- which drops it out of every kind-keyed check */
    fresh_graph(&tg);
    expect_save_err("{\"elements\":[{\"name\":\"Bio Weapon\",\"kind\":\"spell\"},"
                    "{\"name\":\"Bio Weapon\",\"kind\":\"nothing resolves on cast\"}]}",
                    ERR_PROSE_VALUE, "elements[1].kind");
    /* the real kind is untouched by the rejected resubmit */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"Bio Weapon\",\"kind\":\"spell\"}]}"), failed);
    CHECK(!failed);
    expect_save_err("{\"elements\":[{\"name\":\"Bio Weapon\","
                    "\"kind\":\"nothing resolves on cast\"}]}",
                    ERR_PROSE_VALUE, "elements[0].kind");
    CHECK(tg.elem_kind[elem_by_name(&tg, "Bio Weapon")] ==
          elem_by_name(&tg, "spell"));
    unsetenv("LEGEND_NOW");
}

static void test_supersession_chain(void) {
    int failed;
    u32 speed, val, curval, v5, v7, cache1, cache2, event2;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    /* no prior: cache minted with no flip; the event carries no from slot */
    TRY(run_save("{\"changes\":[{\"target\":\"speed\",\"property\":\"value\",\"to\":\"5\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 2);
    speed = elem_by_name(&tg, "speed");
    val = elem_by_name(&tg, "value");
    curval = elem_by_name(&tg, "current_value");
    v5 = elem_by_name(&tg, "5");
    CHECK(speed != NONE_U32 && curval != NONE_U32 && v5 != NONE_U32);
    /* mint order: target, property, to, current_<property> (README a.4) */
    CHECK(twr.minted_elems.count == 4);
    CHECK(twr.minted_elems.v[0] == speed && twr.minted_elems.v[1] == val);
    CHECK(twr.minted_elems.v[2] == v5 && twr.minted_elems.v[3] == curval);
    {
        u32 event1 = twr.minted_rels.v[0];
        cache1 = twr.minted_rels.v[1];
        CHECK(tg.relations[event1].attr_count == 4); /* subject,target,property,to */
        CHECK(cache1 == rel_find_pair(&tg, curval, speed, v5));
        CHECK(cur_get_live(&tg, speed, curval) == cache1);
        CHECK(tg.relations[cache1].stats.confidence == 0.7);
        CHECK(tg.relations[cache1].stats.salience == 0.5); /* novel, no flip */
        CHECK(has_rel_meta(&tg, cache1, WK_DERIVED_FROM, event1));
        CHECK(twr.conflict_count == 0);
    }

    /* prior exists: from-fill, flip, supersedes meta, 0.7 salience seed */
    setenv("LEGEND_NOW", "1780358400", 1);
    TRY(run_save("{\"changes\":[{\"target\":\"speed\",\"property\":\"value\",\"to\":\"7\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 2);
    v7 = elem_by_name(&tg, "7");
    event2 = twr.minted_rels.v[0];
    cache2 = twr.minted_rels.v[1];
    CHECK(tg.relations[event2].attr_count == 5); /* from-filled */
    {
        u32 a;
        int from_is_5 = 0;
        for (a = 0; a < 5; a++)
            if (tg.relations[event2].attrs[a].name == WK_FROM &&
                tg.relations[event2].attrs[a].value.id == v5)
                from_is_5 = 1;
        CHECK(from_is_5);
    }
    /* the cache-filled from counts as a write-position touch (spec §7) */
    CHECK(twr.reused_elems.count == 2);
    CHECK(twr.reused_elems.v[0] == speed && twr.reused_elems.v[1] == v5);
    CHECK(tg.relations[cache1].status == ST_SUPERSEDED);
    CHECK(cache2 == rel_find_pair(&tg, curval, speed, v7));
    CHECK(cur_get_live(&tg, speed, curval) == cache2);
    CHECK(tg.relations[cache2].stats.salience == 0.7); /* supersession seed */
    CHECK(has_rel_meta(&tg, cache2, WK_SUPERSEDES, cache1));
    CHECK(has_rel_meta(&tg, cache2, WK_DERIVED_FROM, event2));

    /* gate fail with a contradicting live prior: conflict, no flip, 0.9 seed */
    setenv("LEGEND_NOW", "1780445000", 1);
    {
        u32 rels_before = tg.relation_count;
        TRY(run_save("{\"changes\":[{\"target\":\"speed\",\"property\":\"value\",\"to\":\"9\",\"confidence\":0.1}]}"), failed);
        CHECK(!failed);
        CHECK(twr.conflict_count == 1);
        CHECK(twr.conflicts[0].prior_rel == cache2);
        CHECK(twr.conflicts[0].property_elem == val);
        CHECK(twr.conflicts[0].pval_id == v7);
        CHECK(elem_name_is(&tg, twr.conflicts[0].to_val, "9"));
        CHECK(tg.relations[cache2].status == ST_ASSERTED); /* did not flip */
        CHECK(tg.relations[cache2].stats.salience == 0.9); /* conflict seed */
        CHECK(cur_get_live(&tg, speed, curval) == cache2); /* no new cache */
        CHECK(tg.relation_count == rels_before + 1); /* the event only */
        CHECK(twr.minted_rels.count == 1 && twr.minted_rels.v[0] == twr.conflicts[0].event_rel);
    }

    /* intervened bypasses the gate at any confidence */
    setenv("LEGEND_NOW", "1780531200", 1);
    TRY(run_save("{\"changes\":[{\"target\":\"speed\",\"property\":\"value\",\"to\":\"9\","
                 "\"confidence\":0.1,\"intervened\":true}]}"), failed);
    CHECK(!failed && twr.conflict_count == 0);
    CHECK(tg.relations[cache2].status == ST_SUPERSEDED);
    {
        u32 v9 = elem_by_name(&tg, "9");
        u32 cache3 = rel_find_pair(&tg, curval, speed, v9);
        CHECK(cache3 != NONE_U32 && cur_get_live(&tg, speed, curval) == cache3);
        CHECK(tg.relations[cache3].stats.confidence == 0.1);
    }

    /* prediction_error modulates the gate: eff = 0.35 * (1 - pe) */
    setenv("LEGEND_NOW", "1780617600", 1);
    TRY(run_save("{\"intent\":{\"prediction_error\":0.9},"
                 "\"changes\":[{\"target\":\"speed\",\"property\":\"value\",\"to\":\"11\","
                 "\"confidence\":0.1}]}"), failed);
    CHECK(!failed && twr.conflict_count == 0); /* 0.1 >= 0.35*0.1 */
    CHECK(elem_by_name(&tg, "11") != NONE_U32);

    /* same value re-change: the cache dedup-reuses, nothing flips */
    setenv("LEGEND_NOW", "1780704000", 1);
    {
        u32 v11 = elem_by_name(&tg, "11");
        u32 cache5 = rel_find_pair(&tg, curval, speed, v11);
        TRY(run_save("{\"changes\":[{\"target\":\"speed\",\"property\":\"value\",\"to\":\"11\"}]}"), failed);
        CHECK(!failed && twr.reused_rel_count == 1 && twr.reused_rels[0].rel == cache5);
        CHECK(tg.relations[cache5].status == ST_ASSERTED);
        CHECK(twr.conflict_count == 0);
    }

    /* a same-payload chain supersedes its own staged cache */
    fresh_graph(&tg);
    TRY(run_save("{\"changes\":[{\"target\":\"hp\",\"property\":\"mult\",\"to\":\"2x\"},"
                 "{\"target\":\"hp\",\"property\":\"mult\",\"to\":\"2.4x\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 4);
    {
        u32 hp = elem_by_name(&tg, "hp");
        u32 curm = elem_by_name(&tg, "current_mult");
        u32 c1 = twr.minted_rels.v[1], c2 = twr.minted_rels.v[3];
        CHECK(tg.relations[c1].status == ST_SUPERSEDED);
        CHECK(tg.relations[c2].status == ST_ASSERTED);
        CHECK(cur_get_live(&tg, hp, curm) == c2);
        CHECK(has_rel_meta(&tg, c2, WK_SUPERSEDES, c1));
        /* the second event from-filled from the staged first cache */
        {
            u32 a;
            int from_is_2x = 0;
            u32 e2 = twr.minted_rels.v[2];
            for (a = 0; a < tg.relations[e2].attr_count; a++)
                if (tg.relations[e2].attrs[a].name == WK_FROM &&
                    elem_name_is(&tg, tg.relations[e2].attrs[a].value.id, "2x"))
                    from_is_2x = 1;
            CHECK(from_is_2x);
        }
    }

    /* pin-25 heal: a plain attr on the changing pair is superseded too */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"jump_height\",\"attrs\":{\"value\":\"3.5\"}}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 1);
    {
        u32 plain = twr.minted_rels.v[0];
        TRY(run_save("{\"changes\":[{\"target\":\"jump_height\",\"property\":\"value\",\"to\":\"4.2\"}]}"), failed);
        CHECK(!failed);
        CHECK(tg.relations[plain].status == ST_SUPERSEDED);
        {
            u32 jh = elem_by_name(&tg, "jump_height");
            u32 cv = elem_by_name(&tg, "current_value");
            u32 cache = cur_get_live(&tg, jh, cv);
            CHECK(cache != NONE_U32);
            CHECK(has_rel_meta(&tg, cache, WK_SUPERSEDES, plain));
        }
    }
    unsetenv("LEGEND_NOW");
}

static void test_event_fact_equivalence(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"changes\":[{\"target\":\"ms\",\"property\":\"cost\",\"to\":\"1B\"}]}"), failed);
    CHECK(!failed);
    {
        u32 ms = elem_by_name(&tg, "ms");
        u32 cc = elem_by_name(&tg, "current_cost");
        u32 cache1 = cur_get_live(&tg, ms, cc);
        TRY(run_save("{\"facts\":[{\"attrs\":{\"target\":\"ms\",\"property\":\"cost\","
                     "\"from\":\"1B\",\"to\":\"1G\"},\"confidence\":0.8}]}"), failed);
        CHECK(!failed && twr.minted_rels.count == 2);
        CHECK(tg.relations[cache1].status == ST_SUPERSEDED);
        {
            u32 event = twr.minted_rels.v[0], cache2 = twr.minted_rels.v[1];
            CHECK(tg.relations[event].attr_count == 4); /* submitted slots only */
            CHECK(tg.relations[cache2].stats.confidence == 0.8);
            CHECK(cur_get_live(&tg, ms, cc) == cache2);
            CHECK(has_rel_meta(&tg, cache2, WK_SUPERSEDES, cache1));
        }
        /* the property slot's value is excluded from reused_elements (a.24) */
        {
            u32 i;
            u32 cost = elem_by_name(&tg, "cost");
            for (i = 0; i < twr.reused_elems.count; i++)
                CHECK(twr.reused_elems.v[i] != cost);
        }
    }
    /* an event-shaped fact needs target and property to key the cache */
    expect_save_err("{\"facts\":[{\"attrs\":{\"subject\":\"x\",\"from\":\"1\",\"to\":\"2\"}}]}",
                    ERR_PARSE, "facts[0]");
    expect_save_err("{\"facts\":[{\"attrs\":{\"target\":\"x\",\"property\":\"p\","
                    "\"from\":[\"1\",\"2\"],\"to\":\"3\"}}]}", ERR_PARSE, "facts[0]");
    unsetenv("LEGEND_NOW");
}

static void test_retract(void) {
    int failed;
    volatile u32 rel; /* lives across TRY (setjmp): keep -Wclobbered quiet */
    u32 event, cache;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed);
    rel = twr.minted_rels.v[0];
    TRY(run_save("{\"retract\":[\"rel:10\"]}"), failed);
    CHECK(!failed && rel == 10);
    CHECK(tg.relations[rel].status == ST_RETRACTED);
    CHECK(twr.retracted.count == 1 && twr.retracted.v[0] == rel);
    /* idempotent: still acknowledged, still no error (spec §9) */
    TRY(run_save("{\"retract\":[\"rel:10\"]}"), failed);
    CHECK(!failed && twr.retracted.count == 1 && twr.retracted.v[0] == rel);
    CHECK(tg.relations[rel].status == ST_RETRACTED);
    /* retraction is not a ban: the same fact mints a fresh live relation */
    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 1 && twr.minted_rels.v[0] != rel);

    /* cascade: retracting a change event retracts its derived cache */
    fresh_graph(&tg);
    TRY(run_save("{\"changes\":[{\"target\":\"t\",\"property\":\"v\",\"to\":\"1\"}]}"), failed);
    CHECK(!failed);
    event = twr.minted_rels.v[0];
    cache = twr.minted_rels.v[1];
    TRY(run_save("{\"retract\":[\"rel:10\"]}"), failed);
    CHECK(!failed && event == 10);
    CHECK(tg.relations[event].status == ST_RETRACTED);
    CHECK(tg.relations[cache].status == ST_RETRACTED);
    CHECK(twr.retracted.count == 2);
    CHECK(twr.retracted.v[0] == event && twr.retracted.v[1] == cache);
    /* a retracted cache is no prior: the next change mints with no flip */
    TRY(run_save("{\"changes\":[{\"target\":\"t\",\"property\":\"v\",\"to\":\"2\"}]}"), failed);
    CHECK(!failed && tg.relations[twr.minted_rels.v[0]].attr_count == 4); /* no from-fill */
    CHECK(tg.relations[cache].status == ST_RETRACTED); /* priors never revived */

    /* fact-shape retract: located by attr-set equality among live relations */
    fresh_graph(&tg);
    TRY(run_save("{\"facts\":[{\"s\":\"x\",\"p\":\"uses\",\"o\":\"y\"}]}"), failed);
    CHECK(!failed);
    rel = twr.minted_rels.v[0];
    TRY(run_save("{\"retract\":[{\"s\":\"x\",\"p\":\"uses\",\"o\":\"y\"}]}"), failed);
    CHECK(!failed && twr.retracted.count == 1 && twr.retracted.v[0] == rel);
    CHECK(tg.relations[rel].status == ST_RETRACTED);
    /* already retracted: the fact shape no longer resolves (pin §3.16) */
    expect_save_err("{\"retract\":[{\"s\":\"x\",\"p\":\"uses\",\"o\":\"y\"}]}",
                    ERR_UNKNOWN_REF, "retract[0]");
    expect_save_err("{\"retract\":[{\"s\":\"x\",\"p\":\"never\",\"o\":\"y\"}]}",
                    ERR_UNKNOWN_REF, "retract[0].p");
    expect_save_err("{\"retract\":[\"rel:9999\"]}", ERR_UNKNOWN_REF, "retract[0]");

    /* ambiguity: a homonym in a precise position carries candidates */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"Mercury\",\"kind\":\"planet\"}]}"), failed);
    CHECK(!failed);
    TRY(run_save("{\"elements\":[{\"name\":\"Mercury\",\"kind\":\"person\",\"new\":true}]}"), failed);
    CHECK(!failed);
    TRY(run_save("{\"retract\":[{\"s\":\"Mercury\",\"p\":\"uses\",\"o\":\"z\"}]}"), failed);
    CHECK(failed && g_err.code == ERR_AMBIGUOUS_REF);
    CHECK(strcmp(g_err.at, "retract[0].s") == 0);
    CHECK(strstr(g_err.candidates, "\"name\":\"Mercury\"") != NULL);
    CHECK(strstr(g_err.candidates, "\"score\":1") != NULL);
    unsetenv("LEGEND_NOW");
}

static void test_merge_fold(void) {
    int failed;
    u32 from, into, rel_from, rel_into;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    /* core vocabulary can never be folded away (mirrors the rename guard) */
    expect_save_err("{\"merge\":[{\"from\":\"subject\",\"into\":\"target\"}]}",
                    ERR_PARSE, "merge[0].from");

    TRY(run_save("{\"elements\":[{\"name\":\"colour\"},{\"name\":\"color variant\"}],"
                 "\"facts\":[{\"s\":\"colour\",\"p\":\"uses\",\"o\":\"pigment\"},"
                 "{\"s\":\"color variant\",\"p\":\"uses\",\"o\":\"pigment\"},"
                 "{\"s\":\"color variant\",\"p\":\"part_of\",\"o\":\"palette\"}]}"), failed);
    CHECK(!failed);
    into = elem_by_name(&tg, "colour");
    from = elem_by_name(&tg, "color variant");
    rel_into = twr.minted_rels.v[0];
    rel_from = twr.minted_rels.v[1];
    TRY(run_save("{\"merge\":[{\"from\":\"color variant\",\"into\":\"colour\"}]}"), failed);
    CHECK(!failed && twr.merge_count == 1);
    CHECK(tg.elements[from].redirect == into);
    CHECK(tg.elements[into].redirect == NONE_U32);
    /* names became aliases; the old name resolves to into */
    TRY(run_save("{\"facts\":[{\"s\":\"color variant\",\"p\":\"uses\",\"o\":\"pigment\"}]}"), failed);
    CHECK(!failed && twr.reused_elems.count >= 1 && twr.reused_elems.v[0] == into);
    /* collision collapsed: the duplicate {uses pigment} folded, stats maxed */
    CHECK(tg.relations[rel_into].status == ST_ASSERTED);
    CHECK(tg.relations[rel_from].status == ST_RETRACTED);
    CHECK(tg.relations[rel_into].stats.support_count >= 2); /* reuse bumped the survivor */
    /* the non-colliding relation was rewritten to into */
    {
        u32 palette = elem_by_name(&tg, "palette");
        u32 part = rel_find_pair(&tg, WK_PART_OF, into, palette);
        CHECK(part != NONE_U32);
    }
    /* #from follows the tombstone in payloads */
    {
        char payload[64];
        snprintf(payload, sizeof payload, "{\"facts\":[{\"s\":\"#%u\",\"p\":\"uses\",\"o\":\"ink\"}]}", from);
        TRY(run_save(payload), failed);
        CHECK(!failed && twr.reused_elems.v[0] == into);
    }
    /* idempotent re-merge: echoed, nothing changes */
    TRY(run_save("{\"merge\":[{\"from\":\"color variant\",\"into\":\"colour\"}]}"), failed);
    CHECK(!failed && twr.merge_count == 1);
    CHECK(tg.elements[from].redirect == into);
    /* chained folds stay single-hop */
    TRY(run_save("{\"elements\":[{\"name\":\"hue\"}]}"), failed);
    CHECK(!failed);
    {
        u32 hue = elem_by_name(&tg, "hue");
        char payload[64];
        snprintf(payload, sizeof payload, "{\"merge\":[{\"from\":\"#%u\",\"into\":\"#%u\"}]}", into, hue);
        TRY(run_save(payload), failed);
        CHECK(!failed);
        CHECK(tg.elements[into].redirect == hue);
        CHECK(tg.elements[from].redirect == hue); /* re-pointed, no chain */
    }
    /* precise positions reject homonyms with candidates */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"Mercury\",\"kind\":\"planet\"}]}"), failed);
    CHECK(!failed);
    TRY(run_save("{\"elements\":[{\"name\":\"Mercury\",\"kind\":\"person\",\"new\":true}]}"), failed);
    CHECK(!failed);
    TRY(run_save("{\"merge\":[{\"from\":\"Mercury\",\"into\":\"person\"}]}"), failed);
    CHECK(failed && g_err.code == ERR_AMBIGUOUS_REF);
    CHECK(strcmp(g_err.at, "merge[0].from") == 0);
    CHECK(g_err.candidates[0] == '[');
    expect_save_err("{\"merge\":[{\"from\":\"nonesuch\",\"into\":\"person\"}]}",
                    ERR_UNKNOWN_REF, "merge[0].from");
    unsetenv("LEGEND_NOW");
}

static void test_rename_and_visibility(void) {
    int failed;
    u32 imp;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"elements\":[{\"name\":\"Bark Imp\"}]}"), failed);
    CHECK(!failed);
    imp = elem_by_name(&tg, "Bark Imp");
    CHECK(imp != NONE_U32);
    /* rename_to: new canonical prepended, old kept as alias (pin 23) */
    TRY(run_save("{\"elements\":[{\"name\":\"Bark Imp\",\"rename_to\":\"Mischief Maker\"}]}"), failed);
    CHECK(!failed);
    CHECK(elem_name_is(&tg, imp, "Mischief Maker"));
    CHECK(tg.elements[imp].names.count == 2);
    CHECK(twr.reused_elems.count == 1 && twr.reused_elems.v[0] == imp);
    /* both names resolve */
    TRY(run_save("{\"facts\":[{\"s\":\"Bark Imp\",\"p\":\"uses\",\"o\":\"mischief\"}]}"), failed);
    CHECK(!failed && twr.reused_elems.v[0] == imp);
    TRY(run_save("{\"facts\":[{\"s\":\"mischief-maker\",\"p\":\"uses\",\"o\":\"pranks\"}]}"), failed);
    CHECK(!failed && twr.reused_elems.v[0] == imp);

    /* same-save visibility (pin 24): aliases and renames land for later refs */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"Hellfire\",\"aliases\":[\"Fireball\"]}],"
                 "\"facts\":[{\"s\":\"Fireball\",\"p\":\"costs\",\"o\":\"2R\"}]}"), failed);
    CHECK(!failed);
    {
        u32 hf = elem_by_name(&tg, "Hellfire");
        u32 r2 = elem_by_name(&tg, "2R");
        u32 costs = elem_by_name(&tg, "costs");
        CHECK(hf != NONE_U32 && rel_find_pair(&tg, costs, hf, r2) != NONE_U32);
        CHECK(elem_by_name(&tg, "Fireball") == NONE_U32); /* no second mint */
    }
    TRY(run_save("{\"elements\":[{\"name\":\"Hellfire\",\"rename_to\":\"Cocytus\"}],"
                 "\"facts\":[{\"s\":\"Cocytus\",\"p\":\"costs\",\"o\":\"1B\"}]}"), failed);
    CHECK(!failed);
    {
        u32 hf = elem_by_name(&tg, "Cocytus");
        u32 b1 = elem_by_name(&tg, "1B");
        u32 costs = elem_by_name(&tg, "costs");
        CHECK(hf != NONE_U32 && elem_name_is(&tg, hf, "Cocytus"));
        CHECK(rel_find_pair(&tg, costs, hf, b1) != NONE_U32);
    }

    /* M5 fuzz regression (fuzz/fuzz_payload.py, seed 987654321, it 46287):
     * rename_to on a seeded ontology element re-canonicalized "person", and
     * the snapshot reader — which verifies elements 0..31 by name — then
     * rejected the store the save itself wrote. Plan-phase error now. */
    expect_save_err("{\"source\":\"claude-code:alchamancer2\",\"elements\":[{\"name\":"
                    "\"person\",\"summary\":\"ranged apple-thrower, display name "
                    "Mischief Maker; barkimp sprite prefix and ENEMY_BARK_IMP enum "
                    "unchanged\",\"rename_to\":\"Mischief Maker\"}],\"changes\":"
                    "[{\"target\":\"Bark Imp\",\"property\":\"display_name\",\"from\":"
                    "\"Bark Imp\",\"to\":\"Mischief Maker\",\"intervened\":true,"
                    "\"src\":\"commit c64de8c\"}]}",
                    ERR_PARSE, "elements[0].rename_to");
    expect_save_err("{\"elements\":[{\"name\":\"#0\",\"rename_to\":\"topic\"}]}",
                    ERR_PARSE, "elements[0].rename_to");
    /* a forced homonym mint is not the core element: its rename is fine */
    TRY(run_save("{\"elements\":[{\"name\":\"person\",\"new\":true,"
                 "\"rename_to\":\"protagonist\"}]}"), failed);
    CHECK(!failed && twr.minted_elems.count == 1);
    CHECK(elem_name_is(&tg, twr.minted_elems.v[0], "protagonist"));
    CHECK(elem_name_is(&tg, WK_KIND_PERSON, "person"));
    unsetenv("LEGEND_NOW");
}

static void test_template_drift(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    /* same-save template + instance: only the unexpected attr drifts */
    TRY(run_save("{\"templates\":[{\"kind\":\"character\",\"expects\":[\"role\",\"wants\"]}],"
                 "\"elements\":[{\"name\":\"Vex\",\"kind\":\"character\","
                 "\"attrs\":{\"role\":\"mentor\",\"theme_song\":\"dies irae\"}}]}"), failed);
    CHECK(!failed);
    CHECK(twr.drift_count == 1);
    CHECK(elem_name_is(&tg, twr.drifts[0].elem, "Vex"));
    CHECK(elem_name_is(&tg, twr.drifts[0].kind_elem, "character"));
    CHECK(twr.drifts[0].count == 1);
    CHECK(elem_name_is(&tg, twr.drifts[0].keys[0], "theme_song"));

    /* extending the template on a later save clears the drift */
    TRY(run_save("{\"templates\":[{\"kind\":\"character\",\"expects\":[\"theme_song\"]}],"
                 "\"elements\":[{\"name\":\"Nyx\",\"kind\":\"character\","
                 "\"attrs\":{\"theme_song\":\"toccata\"}}]}"), failed);
    CHECK(!failed && twr.drift_count == 0);
    CHECK(twr.minted_rels.count >= 1); /* the expects extension edge minted */

    /* seeded templates drift too */
    TRY(run_save("{\"elements\":[{\"name\":\"gc\",\"kind\":\"decision\","
                 "\"attrs\":{\"chose\":\"x\",\"mood\":\"grim\"}}]}"), failed);
    CHECK(!failed && twr.drift_count == 1 && twr.drifts[0].count == 1);
    CHECK(elem_name_is(&tg, twr.drifts[0].keys[0], "mood"));

    /* a kind with no template never drifts */
    TRY(run_save("{\"elements\":[{\"name\":\"m1\",\"kind\":\"mechanic\","
                 "\"attrs\":{\"anything\":\"goes\"}}]}"), failed);
    CHECK(!failed && twr.drift_count == 0);
    unsetenv("LEGEND_NOW");
}

static void test_promotion(void) {
    int failed;
    volatile u32 rel; /* lives across TRY (setjmp): keep -Wclobbered quiet */
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"source\":\"alice\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\","
                 "\"status\":\"defeasible\"}]}"), failed);
    CHECK(!failed);
    rel = twr.minted_rels.v[0];
    TRY(run_save("{\"source\":\"alice\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.reused_rels[0].promoted == 0);
    /* support 3 but diversity 1: the gate holds */
    TRY(run_save("{\"source\":\"alice\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.reused_rels[0].support_count == 3);
    CHECK(twr.reused_rels[0].promoted == 0);
    CHECK(tg.relations[rel].status == ST_DEFEASIBLE);
    /* a second source: support 4, diversity 2 -> Asserted */
    TRY(run_save("{\"source\":\"bob\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.reused_rels[0].promoted == 1);
    CHECK(tg.relations[rel].status == ST_ASSERTED);
    /* already promoted: no re-flip report */
    TRY(run_save("{\"source\":\"carol\",\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed && twr.reused_rels[0].promoted == 0);

    /* diversity before support: two sources at support 2 stay Defeasible */
    fresh_graph(&tg);
    TRY(run_save("{\"source\":\"alice\",\"facts\":[{\"s\":\"c\",\"p\":\"uses\",\"o\":\"d\","
                 "\"status\":\"defeasible\"}]}"), failed);
    CHECK(!failed);
    rel = twr.minted_rels.v[0];
    TRY(run_save("{\"source\":\"bob\",\"facts\":[{\"s\":\"c\",\"p\":\"uses\",\"o\":\"d\"}]}"), failed);
    CHECK(!failed && twr.reused_rels[0].promoted == 0);
    CHECK(tg.relations[rel].status == ST_DEFEASIBLE);
    unsetenv("LEGEND_NOW");
}

static void test_salience_seeds(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    /* novel mints seed 0.5 (plan §3.5); caller salience overrides */
    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"},"
                 "{\"s\":\"a\",\"p\":\"likes\",\"o\":\"c\",\"salience\":0.95}]}"), failed);
    CHECK(!failed);
    CHECK(tg.relations[twr.minted_rels.v[0]].stats.salience == 0.5);
    CHECK(tg.relations[twr.minted_rels.v[1]].stats.salience == 0.95);
    CHECK(tg.elements[elem_by_name(&tg, "a")].stats.salience == 0.5);
    /* reuse: the S7 arousal bump lands first (default save arousal 0.3:
     * 0.5 + 0.1*0.3*(1-0.5) = 0.515), then the graph-PE +0.05 -> 0.565 */
    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed);
    {
        double s = tg.relations[twr.reused_rels[0].rel].stats.salience;
        CHECK(s > 0.5649 && s < 0.5651);
    }
    /* payload intent reaches the S7 operator: arousal 1.0 scales the bump
     * (0.565 + 0.1*1.0*(1-0.565) = 0.6085), then the graph-PE +0.05 */
    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}],"
                 "\"intent\":{\"arousal\":1}}"), failed);
    CHECK(!failed);
    {
        double s = tg.relations[twr.reused_rels[0].rel].stats.salience;
        CHECK(s > 0.6584 && s < 0.6586);
    }
    /* supersession 0.7 and conflict 0.9 are asserted in test_supersession_chain */
    unsetenv("LEGEND_NOW");
}

static void test_element_src(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"elements\":[{\"name\":\"jump.rs\",\"src\":\"src/player/jump.rs\"}]}"), failed);
    CHECK(!failed);
    {
        u32 e = elem_by_name(&tg, "jump.rs");
        u32 ptr = elem_by_name(&tg, "src/player/jump.rs");
        u32 base = rel_find_pair(&tg, WK_SRC, e, ptr);
        CHECK(e != NONE_U32 && ptr != NONE_U32);
        CHECK(base != NONE_U32); /* {subject, src: pointer} base relation */
        CHECK(tg.elem_kind[ptr] == WK_KIND_POINTER);
        CHECK(twr.minted_rels.count == 1 && twr.minted_rels.v[0] == base);
        CHECK(twr.ptr_count == 1 && twr.ptrs[0].elem == ptr && twr.ptrs[0].rel == base);
    }
    unsetenv("LEGEND_NOW");
}

static void test_constraint_cache(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"elements\":[{\"name\":\"no laggy input\",\"kind\":\"constraint\"}]}"), failed);
    CHECK(!failed);
    {
        u32 e = elem_by_name(&tg, "no laggy input");
        u32 cs = elem_by_name(&tg, "current_standing");
        u32 cache;
        CHECK(e != NONE_U32 && cs != NONE_U32);
        cache = cur_get_live(&tg, e, cs);
        CHECK(cache != NONE_U32);
        CHECK(elem_name_is(&tg, tg.relations[cache].attrs[1].value.id, "active"));
        /* lifting it is a real supersession flip */
        TRY(run_save("{\"changes\":[{\"target\":\"no laggy input\",\"property\":\"standing\","
                     "\"to\":\"lifted\",\"intervened\":true}]}"), failed);
        CHECK(!failed);
        CHECK(tg.relations[cache].status == ST_SUPERSEDED);
        CHECK(cur_get_live(&tg, e, cs) != cache);
    }
    /* a supplied standing seeds the cache instead of being written beside it:
     * hardcoding active contradicted a constraint declared retired at mint, and
     * constraint_is_active reads only the cache */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"dead rule\",\"kind\":\"constraint\","
                 "\"attrs\":{\"standing\":\"retired\"}}]}"), failed);
    CHECK(!failed);
    {
        u32 e = elem_by_name(&tg, "dead rule");
        u32 cs = elem_by_name(&tg, "current_standing");
        u32 cache, r;
        CHECK(e != NONE_U32 && cs != NONE_U32);
        cache = cur_get_live(&tg, e, cs);
        CHECK(cache != NONE_U32);
        CHECK(elem_name_is(&tg, tg.relations[cache].attrs[1].value.id, "retired"));
        CHECK(!constraint_is_active(&tg, e));
        /* and no second copy lingers as a plain standing fact */
        for (r = 0; r < tg.relation_count; r++) {
            const Relation *rel = &tg.relations[r];
            if (rel->status >= ST_SUPERSEDED || rel->attr_count != 2)
                continue;
            CHECK(!(rel->attrs[0].value.id == e && rel->attrs[1].name == WK_STANDING));
        }
    }
    /* the current_* cache is the change path's to write: a plain fact there sits
     * beside it instead of superseding it, and the audit skips the prefix, so
     * two contradicting values would read as clean */
    fresh_graph(&tg);
    expect_save_err("{\"facts\":[{\"s\":\"brakes\",\"p\":\"current_standing\","
                    "\"o\":\"active\"}]}",
                    ERR_PARSE, "facts[0].p");
    fresh_graph(&tg);
    expect_save_err("{\"elements\":[{\"name\":\"X\",\"attrs\":{\"current_phase\":\"two\"}}]}",
                    ERR_PARSE, "elements[0].attrs.current_phase");
    /* changes still write it */
    fresh_graph(&tg);
    TRY(run_save("{\"changes\":[{\"target\":\"brakes\",\"property\":\"standing\","
                 "\"to\":\"settled\"}]}"), failed);
    CHECK(!failed);
    CHECK(cur_get_live(&tg, elem_by_name(&tg, "brakes"),
                       elem_by_name(&tg, "current_standing")) != NONE_U32);
    unsetenv("LEGEND_NOW");
}

static void test_recall_tick(void) {
    int failed;
    static Recall rec;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"facts\":[{\"s\":\"engine\",\"p\":\"uses\",\"o\":\"steam\"}]}"), failed);
    CHECK(!failed && tg.clock == 1);
    {
        Rd r;
        const char *q = "{\"focus\":[\"engine\",\"nonesuch\"]}";
        size_t n = strlen(q);
        memcpy(tb, q, n + 1);
        json_parse(&tj, tb, (u32)n);
        r.t = tj.toks;
        r.buf = tb;
        read_recall(&r, &rec);
        setenv("LEGEND_NOW", "1780358400", 1);
        TRY(tick_recall(&tg, &rec, tb, &twr), failed);
        CHECK(!failed);
        CHECK(twr.tick == 2 && tg.clock == 2); /* recall advances the clock */
        CHECK(strcmp(twr.focus_query, "engine nonesuch") == 0); /* defaults to joined focus */
        CHECK(twr.focus_elems.count == 1);
        CHECK(elem_name_is(&tg, twr.focus_elems.v[0], "engine"));
        CHECK(tg.elements[twr.focus_elems.v[0]].stats.last_seen == 2);
        CHECK(twr.res_count == 1 && twr.res[0].resolved == 0); /* nonesuch */
        CHECK(strcmp(twr.res[0].at, "focus[1]") == 0);
        /* observe: no clock advance, no bumps */
        rec.observe = 1;
        TRY(tick_recall(&tg, &rec, tb, &twr), failed);
        CHECK(!failed && twr.tick == 2 && tg.clock == 2);
        /* an explicit `query` overrides the focus-join as the ranking signal
         * without touching element resolution (query is never resolved) */
        {
            const char *q2 = "{\"focus\":[\"engine\"],\"query\":\"what fuel does it use\"}";
            size_t n2 = strlen(q2);
            memcpy(tb, q2, n2 + 1);
            json_parse(&tj, tb, (u32)n2);
            r.t = tj.toks;
            r.buf = tb;
            read_recall(&r, &rec);
            TRY(tick_recall(&tg, &rec, tb, &twr), failed);
            CHECK(!failed);
            CHECK(twr.focus_elems.count == 1 &&
                  elem_name_is(&tg, twr.focus_elems.v[0], "engine"));
            CHECK(strcmp(twr.focus_query, "what fuel does it use") == 0);
        }
    }
    unsetenv("LEGEND_NOW");
}

/* --------------------------- M3 frame tests --------------------------- */

static Recall t_rec;

static void run_recall_on(Hypergraph *g, const char *payload) {
    Rd r;
    size_t n = strlen(payload);
    memcpy(tb, payload, n + 1);
    json_parse(&tj, tb, (u32)n);
    r.t = tj.toks;
    r.buf = tb;
    read_recall(&r, &t_rec);
    tick_recall(g, &t_rec, tb, &twr);
}

static void run_recall(const char *payload) { run_recall_on(&tg, payload); }

/* The frame JSON for the current report, via legend.c's own capture path. */
static char t_frame[1 << 16];

static void capture_frame(i64 limit, i32 history_depth, i64 since) {
    u32 len;
    char *buf = capture_frame_json(&tg, &twr, "/t", limit, history_depth, since, &len);
    if (len >= sizeof t_frame) len = sizeof t_frame - 1;
    memcpy(t_frame, buf, len);
    t_frame[len] = 0;
    free(buf);
}

static u32 count_occ(const char *hay, const char *needle) {
    u32 n = 0;
    const char *p = hay;
    while ((p = strstr(p, needle)) != NULL) { n++; p += strlen(needle); }
    return n;
}

/* A statement nested as the CONTENT of another: the store has always held it,
 * the frame used to render it as a bare pointer (see frame_slot_nests). */
/* The summary wall. Instruction-only guidance ran at 50% compliance on the
 * trial store; this is the changes.to backstop's pattern applied to the one
 * health metric that degrades once normalized for growth. */
static void test_summary_cap(void) {
    char pay[1400];
    char big[900];
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    memset(big, 'x', sizeof big - 1);
    big[sizeof big - 1] = 0;

    /* at the cap passes, one over is rejected -- the boundary is exact */
    big[g_sum_chars] = 0;
    snprintf(pay, sizeof pay,
             "{\"elements\":[{\"name\":\"at cap\",\"summary\":\"%s\"}]}", big);
    TRY(run_save(pay), failed);
    CHECK(!failed);

    big[g_sum_chars] = 'x';
    big[g_sum_chars + 1] = 0;
    snprintf(pay, sizeof pay,
             "{\"elements\":[{\"name\":\"over cap\",\"summary\":\"%s\"}]}", big);
    TRY(run_save(pay), failed);
    CHECK(failed && g_err.code == ERR_PROSE_VALUE);

    /* it applies on RESUBMIT too, not only at mint: a resubmit overwrites the
     * summary, so a long one there does exactly the same damage */
    TRY(run_save("{\"elements\":[{\"name\":\"grower\",\"summary\":\"short\"}]}"), failed);
    CHECK(!failed);
    snprintf(pay, sizeof pay,
             "{\"elements\":[{\"name\":\"grower\",\"summary\":\"%s\"}]}", big);
    TRY(run_save(pay), failed);
    CHECK(failed && g_err.code == ERR_PROSE_VALUE);

    /* an element with no summary at all is untouched */
    TRY(run_save("{\"elements\":[{\"name\":\"bare\",\"kind\":\"mechanic\"}]}"), failed);
    CHECK(!failed);
}

static void test_nested_statement(void) {
    u32 inner, outer;
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"elements\":[{\"name\":\"Nick\",\"kind\":\"person\"}],"
                 "\"facts\":[{\"attrs\":{\"subject\":\"me\",\"removes\":\"em dash\","
                 "\"from\":\"prose\"},\"modal\":[\"non_actual\"]}]}"),
        failed);
    CHECK(!failed && twr.minted_rels.count >= 1);
    inner = twr.minted_rels.v[0];

    {
        char pay[192];
        snprintf(pay, sizeof pay,
                 "{\"facts\":[{\"attrs\":{\"subject\":\"Nick\",\"asked\":\"me\","
                 "\"content\":\"rel:%u\"}}]}", inner);
        TRY(run_save(pay), failed);
    }
    CHECK(!failed && twr.minted_rels.count == 1);
    outer = twr.minted_rels.v[0];
    CHECK(outer != inner);

    /* the frame expands the inner statement AND carries its modal -- rendered
     * bare, a non_actual claim reads as a plain assertion */
    TRY(run_recall("{\"focus\":[\"Nick\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"content\":{\"ref\":\"rel:") != NULL);
    CHECK(strstr(t_frame, "\"modal\":[\"non_actual\"]") != NULL);
    CHECK(strstr(t_frame, "\"content\":\"rel:") == NULL); /* no bare pointer */

    /* a container is reachable from a term appearing ONLY in the inner
     * statement, else the request is invisible from the term the reader has */
    TRY(run_recall("{\"focus\":[\"em dash\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"asked\":\"me\"") != NULL);

    /* the nesting is one slot on the outer relation, not a copy */
    CHECK(tg.relations[outer].attr_count == 3);
}

static void test_tier2_read(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    /* the containment math itself */
    {
        U32Vec q = { 0, 0, 0 }, t = { 0, 0, 0 };
        trigram_set_build("jump feel", 9, &q);
        trigram_set_build("jump feel system", 16, &t);
        CHECK(trigram_containment(q.v, q.count, t.v, t.count) == 1.0);
        trigram_set_build("alpha beta", 10, &q);
        trigram_set_build("alpha", 5, &t);
        CHECK(trigram_containment(q.v, q.count, t.v, t.count) == 0.375); /* 3/8 */
        CHECK(trigram_containment(NULL, 0, t.v, t.count) == 0.0);
        free(q.v);
        free(t.v);
    }

    TRY(run_save("{\"elements\":[{\"name\":\"jump_physics\",\"kind\":\"concept\","
                 "\"summary\":\"the platformer's jump feel system\"}]}"), failed);
    CHECK(!failed);

    /* a focus phrase buried in a summary resolves via lexical, score 1 */
    TRY(run_recall("{\"focus\":[\"jump feel\"]}"), failed);
    CHECK(!failed);
    CHECK(twr.focus_elems.count == 1 &&
          elem_name_is(&tg, twr.focus_elems.v[0], "jump_physics"));
    CHECK(twr.res_count == 1 && twr.res[0].resolved);
    CHECK(twr.res[0].via == VIA_LEXICAL && twr.res[0].score == 1.0);
    CHECK(strcmp(twr.res[0].at, "focus[0]") == 0);

    /* threshold edge: [0.3, 0.6) is a lexical candidate, never auto-resolves;
     * the salience roster backfills behind it (score 0) for the caller to scan */
    TRY(run_save("{\"elements\":[{\"name\":\"alpha\"}]}"), failed);
    CHECK(!failed);
    TRY(run_recall("{\"focus\":[\"alpha beta\"]}"), failed);
    CHECK(!failed);
    CHECK(twr.focus_elems.count == 0);
    CHECK(twr.res_count == 1 && !twr.res[0].resolved);
    CHECK(twr.res[0].cand_count >= 1);
    CHECK(elem_name_is(&tg, twr.cands.v[twr.res[0].cand_start].elem, "alpha"));
    CHECK(twr.cands.v[twr.res[0].cand_start].score == 0.375);

    /* below 0.3: no lexical match, but the salience roster still backfills so
     * the caller gets a list to scan (roster entries carry score 0) */
    TRY(run_recall("{\"focus\":[\"zqxwv\"]}"), failed);
    CHECK(!failed);
    CHECK(twr.res_count == 1 && !twr.res[0].resolved);
    CHECK(twr.res[0].cand_count > 0);
    CHECK(twr.cands.v[twr.res[0].cand_start].score == 0.0);

    /* several matches tied at the top are ambiguous: no auto-resolve, they all
     * report as candidates (score desc then id asc), the roster behind them */
    TRY(run_save("{\"elements\":[{\"name\":\"jump feel one\"},{\"name\":\"jump feel two\"}]}"), failed);
    CHECK(!failed);
    TRY(run_recall("{\"focus\":[\"jump feel\"]}"), failed);
    CHECK(!failed);
    CHECK(twr.focus_elems.count == 0);
    CHECK(twr.res_count == 1 && !twr.res[0].resolved);
    CHECK(twr.res[0].cand_count >= 3); /* summary hit + both names, then roster */
    CHECK(twr.cands.v[twr.res[0].cand_start + 0].score == 1.0 &&
          twr.cands.v[twr.res[0].cand_start + 1].score == 1.0 &&
          twr.cands.v[twr.res[0].cand_start + 2].score == 1.0);
    CHECK(twr.cands.v[twr.res[0].cand_start + 0].elem <
              twr.cands.v[twr.res[0].cand_start + 1].elem &&
          twr.cands.v[twr.res[0].cand_start + 1].elem <
              twr.cands.v[twr.res[0].cand_start + 2].elem);

    /* check-while-writing: a save's focus resolves through tier 2 too */
    TRY(run_save("{\"facts\":[{\"s\":\"gravity\",\"p\":\"part_of\",\"o\":\"jump_physics\"}],"
                 "\"focus\":[\"the platformer s jump feel system\"]}"), failed);
    CHECK(!failed);
    CHECK(twr.focus_elems.count >= 1 &&
          elem_name_is(&tg, twr.focus_elems.v[0], "jump_physics"));
    unsetenv("LEGEND_NOW");
}

static void test_near_matches(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"elements\":[{\"name\":\"jump physics\"}]}"), failed);
    CHECK(!failed && twr.near_count == 0);

    /* a mint scoring >= 0.6 against a pre-existing element reports */
    TRY(run_save("{\"facts\":[{\"s\":\"jump physic\",\"p\":\"uses\",\"o\":\"widget\"}]}"), failed);
    CHECK(!failed);
    CHECK(twr.near_count == 1);
    CHECK(strcmp(twr.nears[0].at, "facts[0].s") == 0);
    CHECK(elem_name_is(&tg, twr.nears[0].minted, "jump physic"));
    CHECK(elem_name_is(&tg, twr.nears[0].existing, "jump physics"));
    CHECK(twr.nears[0].score == 0.9); /* 9 shared of 10 union */

    /* a new:true mint is exempt — the caller explicitly forced it */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"jump physics\"}]}"), failed);
    CHECK(!failed);
    TRY(run_save("{\"elements\":[{\"name\":\"jump physic\",\"new\":true}]}"), failed);
    CHECK(!failed && twr.near_count == 0);

    /* same-tick twins never self-report (pre-tick elements only) */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"color scheme\"},{\"name\":\"colour scheme\"}]}"), failed);
    CHECK(!failed && twr.near_count == 0);

    /* reuses never report — only mints */
    TRY(run_save("{\"facts\":[{\"s\":\"color scheme\",\"p\":\"uses\",\"o\":\"paint\"}]}"), failed);
    CHECK(!failed && twr.near_count == 0);
    unsetenv("LEGEND_NOW");
}

static void test_section_filters(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    /* open: a question is open until a live resolves points at it */
    TRY(run_save("{\"elements\":[{\"name\":\"q1\",\"kind\":\"question\"}]}"), failed);
    CHECK(!failed);
    {
        volatile u32 q1 = elem_by_name(&tg, "q1");
        CHECK(q1 != NONE_U32 && elem_is_open(&tg, q1) == 1);
        TRY(run_recall("{\"focus\":[\"q1\"]}"), failed);
        CHECK(!failed);
        capture_frame(40, 2, -1);
        CHECK(strstr(t_frame, "\"open\":[{\"ref\"") != NULL);
        TRY(run_save("{\"elements\":[{\"name\":\"d1\",\"kind\":\"decision\","
                     "\"attrs\":{\"resolves\":\"q1\"}}]}"), failed);
        CHECK(!failed);
        CHECK(elem_is_open(&tg, q1) == 0);
        TRY(run_recall("{\"focus\":[\"q1\"]}"), failed);
        CHECK(!failed);
        capture_frame(40, 2, -1);
        CHECK(strstr(t_frame, "\"open\":[]") != NULL);
        CHECK(strstr(t_frame, "\"decisions\":[{\"ref\"") != NULL); /* d1 links in */
    }

    /* constraints: current_standing != active leaves the section */
    TRY(run_save("{\"elements\":[{\"name\":\"c1\",\"kind\":\"constraint\","
                 "\"attrs\":{\"applies_to\":\"q1\"}}]}"), failed);
    CHECK(!failed);
    {
        volatile u32 c1 = elem_by_name(&tg, "c1");
        CHECK(c1 != NONE_U32 && constraint_is_active(&tg, c1) == 1);
        TRY(run_recall("{\"focus\":[\"c1\"]}"), failed);
        CHECK(!failed);
        capture_frame(40, 2, -1);
        /* denormalized instance shape: the cache renders as standing=active */
        CHECK(strstr(t_frame, "\"standing\":\"active\"") != NULL);
        TRY(run_save("{\"changes\":[{\"target\":\"c1\",\"property\":\"standing\","
                     "\"to\":\"lifted\",\"intervened\":true}]}"), failed);
        CHECK(!failed);
        CHECK(constraint_is_active(&tg, c1) == 0);
        TRY(run_recall("{\"focus\":[\"c1\"]}"), failed);
        CHECK(!failed);
        capture_frame(40, 2, -1);
        CHECK(strstr(t_frame, "\"constraints\":[]") != NULL);
    }
    unsetenv("LEGEND_NOW");
}

static void test_history_since_limit(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1); /* 2026-06-01 */
    TRY(run_save("{\"changes\":[{\"target\":\"gauge\",\"property\":\"value\",\"to\":\"1\"}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1780617600", 1); /* 06-05 */
    TRY(run_save("{\"changes\":[{\"target\":\"gauge\",\"property\":\"value\",\"to\":\"2\"}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1781049600", 1); /* 06-10 */
    TRY(run_save("{\"changes\":[{\"target\":\"gauge\",\"property\":\"value\",\"to\":\"3\"}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1781913600", 1); /* 06-20 */
    TRY(run_save("{\"changes\":[{\"target\":\"gauge\",\"property\":\"value\",\"to\":\"4\"}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1782777600", 1); /* 06-30 */
    TRY(run_recall("{\"focus\":[\"gauge\"]}"), failed);
    CHECK(!failed);

    /* history_depth: hops back from the live cache; null (-1) = full chain */
    capture_frame(40, 1, -1);
    CHECK(count_occ(t_frame, "superseded_by") == 1);
    capture_frame(40, 2, -1);
    CHECK(count_occ(t_frame, "superseded_by") == 2);
    capture_frame(40, 0, -1);
    CHECK(count_occ(t_frame, "superseded_by") == 0);
    capture_frame(40, -1, -1);
    CHECK(count_occ(t_frame, "superseded_by") == 3);
    CHECK(count_occ(t_frame, "\"property\":\"value\"") == 4); /* all four events in recent */

    /* since filters recent (by date) and history (by asserted date), inclusive */
    capture_frame(40, -1, 1780617600); /* 2026-06-05 */
    CHECK(count_occ(t_frame, "\"property\":\"value\"") == 3); /* e2..e4 */
    CHECK(count_occ(t_frame, "superseded_by") == 2);          /* c2, c3 */
    capture_frame(40, -1, 1781049600); /* 06-10 */
    CHECK(count_occ(t_frame, "\"property\":\"value\"") == 2);
    CHECK(count_occ(t_frame, "superseded_by") == 1);

    /* limit: state and history always included, recent fills the remainder */
    capture_frame(2, -1, -1); /* 1 state + 3 history already exceed it */
    CHECK(strstr(t_frame, "\"recent\":[]") != NULL);
    CHECK(count_occ(t_frame, "superseded_by") == 3);
    capture_frame(6, -1, -1); /* remainder 2: the two newest events */
    CHECK(count_occ(t_frame, "\"property\":\"value\"") == 2);
    CHECK(strstr(t_frame, "\"to\":\"4\"") != NULL);
    CHECK(strstr(t_frame, "\"from\":\"1\"") == NULL); /* e2 (to 2) dropped */
    capture_frame(-1, -1, -1); /* null = uncapped */
    CHECK(count_occ(t_frame, "\"property\":\"value\"") == 4);
    unsetenv("LEGEND_NOW");
}

static void test_related_ranking(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    /* B mints first (lower id); A then earns a focus success. RRF must rank
     * A over B — id-order would put B first, so the formula is load-bearing. */
    TRY(run_save("{\"facts\":[{\"s\":\"hub\",\"p\":\"pb\",\"o\":\"bb\"},"
                 "{\"s\":\"hub\",\"p\":\"pa\",\"o\":\"aa\"}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1780358400", 1);
    TRY(run_recall("{\"focus\":[\"aa\"]}"), failed); /* bumps A's focus_success_count */
    CHECK(!failed);
    setenv("LEGEND_NOW", "1780444800", 1);
    TRY(run_save("{\"facts\":[{\"s\":\"hub\",\"p\":\"pc\",\"o\":\"cc\"}],\"focus\":[\"hub\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    {
        const char *ra = strstr(t_frame, "\"related\":[");
        const char *pa = ra ? strstr(ra, "\"pa\":") : NULL;
        const char *pb = ra ? strstr(ra, "\"pb\":") : NULL;
        CHECK(ra && pa && pb && pa < pb);
        /* the new fact is recent, not related */
        CHECK(strstr(t_frame, "\"recent\":[{\"ref\"") != NULL);
        CHECK(strstr(ra, "\"pc\":") == NULL);
    }
    unsetenv("LEGEND_NOW");
}

static void test_orientation_frame(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"elements\":[{\"name\":\"proj1\",\"kind\":\"project\",\"summary\":\"the project\"},"
                 "{\"name\":\"c1\",\"kind\":\"constraint\"},{\"name\":\"q1\",\"kind\":\"question\"}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1780358400", 1);
    TRY(run_recall("{}"), failed);
    CHECK(!failed);
    CHECK(twr.orientation == 1 && twr.focus_elems.count == 0);
    CHECK(twr.tick == 2);
    capture_frame(40, 2, -1);
    /* 43 seeds (32 core + 10 ext + self) + proj1/c1/q1 + current_standing/active */
    CHECK(strstr(t_frame, "\"overview\":{\"elements\":48,\"relations\":") != NULL);
    CHECK(strstr(t_frame, "\"clock\":2") != NULL);
    CHECK(strstr(t_frame, "\"scope\":{\"ref\":\"#43\",\"name\":\"proj1\",\"kind\":\"project\","
                          "\"summary\":\"the project\"}") != NULL);
    CHECK(strstr(t_frame, "\"focus\":[") == NULL); /* overview replaces focus */
    CHECK(strstr(t_frame, "\"active\":[{\"ref\"") != NULL);
    CHECK(strstr(t_frame, "\"constraints\":[{\"ref\"") != NULL);
    CHECK(strstr(t_frame, "\"standing\":\"active\"") != NULL);
    CHECK(strstr(t_frame, "\"open\":[{\"ref\"") != NULL);
    /* current_standing (attr vocabulary) never ranks in active */
    CHECK(strstr(t_frame, "\"name\":\"current_standing\"") == NULL);

    /* orientation state is store-wide: the standing cache appears */
    CHECK(strstr(t_frame, "\"state\":[{\"ref\"") != NULL);

    /* no project element -> scope null */
    fresh_graph(&tg);
    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed);
    TRY(run_recall("{}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"scope\":null") != NULL);
    CHECK(strstr(t_frame, "\"recent\":[{\"ref\"") != NULL); /* store-wide recent */
    unsetenv("LEGEND_NOW");
}

static void test_observe_identity(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"elements\":[{\"name\":\"thing\",\"summary\":\"a thing\"}],"
                 "\"facts\":[{\"s\":\"thing\",\"p\":\"uses\",\"o\":\"stuff\"}]}"), failed);
    CHECK(!failed);
    snapshot_serialize(&tg, &tbb1);
    setenv("LEGEND_NOW", "1780358400", 1);
    /* observe advances nothing: clock, stamps, stats, resolution all inert */
    TRY(run_recall("{\"focus\":[\"thing\"],\"observe\":true}"), failed);
    CHECK(!failed && twr.tick == 1 && tg.clock == 1);
    TRY(run_recall("{\"observe\":true}"), failed); /* orientation observe too */
    CHECK(!failed && twr.tick == 1 && tg.clock == 1);
    snapshot_serialize(&tg, &tbb2);
    CHECK(tbb1.len == tbb2.len && memcmp(tbb1.v, tbb2.v, tbb1.len) == 0);
    /* and a plain recall does advance the store */
    TRY(run_recall("{\"focus\":[\"thing\"]}"), failed);
    CHECK(!failed && tg.clock == 2);
    snapshot_serialize(&tg, &tbb2);
    CHECK(!(tbb1.len == tbb2.len && memcmp(tbb1.v, tbb2.v, tbb1.len) == 0));
    unsetenv("LEGEND_NOW");
}

static void test_custom_kind_section(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"templates\":[{\"kind\":\"character\",\"expects\":[\"role\",\"wants\"]}],"
                 "\"elements\":[{\"name\":\"Maren\",\"kind\":\"character\","
                 "\"attrs\":{\"role\":\"mentor\",\"wants\":[\"peace\",\"quiet\"]}}]}"), failed);
    CHECK(!failed);
    TRY(run_recall("{\"focus\":[\"Maren\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"character\":[{\"ref\"") != NULL);
    CHECK(strstr(t_frame, "\"wants\":[\"peace\",\"quiet\"]") != NULL); /* id-ascending array */
    CHECK(strstr(t_frame, "\"recent\":[]") != NULL); /* attrs consumed by the section */
    /* a kind with no template gets no section */
    TRY(run_save("{\"elements\":[{\"name\":\"m1\",\"kind\":\"mechanic\","
                 "\"attrs\":{\"speed\":\"9\"}}]}"), failed);
    CHECK(!failed);
    TRY(run_recall("{\"focus\":[\"m1\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"mechanic\":[") == NULL);
    CHECK(strstr(t_frame, "\"speed\":\"9\"") != NULL); /* the attr surfaces in recent */
    unsetenv("LEGEND_NOW");
}

/* --------------------------- M4 dynamics tests --------------------------- */

/* Advance the store clock so LEGEND_NOW moves one day per tick; the dynamics
 * run on ticks, so any monotone stamp works — days keep dates readable. */
static void dyn_set_now(u32 day) {
    char buf[32];
    snprintf(buf, sizeof buf, "%lld", 1780272000LL + (i64)day * 86400);
    setenv("LEGEND_NOW", buf, 1);
}

/* Spaced-vs-massed (spec §3.1, pin 6): same touch count, different spacing ->
 * different stability (x1.3 vs x1.05 per qualifying interval), and the spaced
 * element survives the same number of decay ticks with more activation. */
static void test_dyn_spaced_vs_massed(void) {
    int failed;
    volatile u32 day = 1, t; /* live across TRY (setjmp): keep -Wclobbered quiet */
    fresh_graph(&tg);
    dyn_set_now(day++);
    TRY(run_save("{\"facts\":[{\"s\":\"hub\",\"p\":\"uses\",\"o\":\"mm\"},"
                 "{\"s\":\"hub\",\"p\":\"uses\",\"o\":\"ss\"},"
                 "{\"s\":\"hub\",\"p\":\"uses\",\"o\":\"tt\"}]}"), failed);
    CHECK(!failed);
    /* ss touched at ticks 2, 6, 14 (growing intervals); mm at 3, 4, 5
     * (massed); tt recalls are decay ticks that reach both mm and ss at
     * depth 1 without touching them */
    dyn_set_now(day++);
    TRY(run_recall("{\"focus\":[\"ss\"]}"), failed);           /* tick 2: init */
    CHECK(!failed);
    for (t = 3; t <= 5; t++) {                                 /* ticks 3-5 */
        dyn_set_now(day++);
        TRY(run_recall("{\"focus\":[\"mm\"]}"), failed);
        CHECK(!failed);
    }
    dyn_set_now(day++);
    TRY(run_recall("{\"focus\":[\"ss\"]}"), failed);           /* tick 6: spaced */
    CHECK(!failed);
    for (t = 7; t <= 13; t++) {
        dyn_set_now(day++);
        TRY(run_recall("{\"focus\":[\"tt\"]}"), failed);
        CHECK(!failed);
    }
    dyn_set_now(day++);
    TRY(run_recall("{\"focus\":[\"ss\"]}"), failed);           /* tick 14: spaced */
    CHECK(!failed);
    for (t = 15; t <= 20; t++) {
        dyn_set_now(day++);
        TRY(run_recall("{\"focus\":[\"tt\"]}"), failed);
        CHECK(!failed);
    }
    {
        const Stats *sm = &tg.elements[elem_by_name(&tg, "mm")].stats;
        const Stats *ss = &tg.elements[elem_by_name(&tg, "ss")].stats;
        CHECK(sm->focus_success_count == 3 && ss->focus_success_count == 3);
        /* mm: two massed intervals -> 1.05^2; ss: two spaced -> 1.3^2 */
        CHECK(sm->stability > 1.1024 && sm->stability < 1.1026);
        CHECK(ss->stability > 1.6899 && ss->stability < 1.6901);
        /* equal decay counts (16 each), but stability divides the rate */
        CHECK(ss->activation > sm->activation);
        CHECK(sm->activation > 0.0 && sm->activation < 1.0);
    }
    unsetenv("LEGEND_NOW");
}

/* Importance effect-topology (spec §3.1): caller salience seeds the element,
 * divides its decay rate (high-salience outlives equal-touch low-salience in
 * activation), and CPEB-bumps the stability of same-tick mints that touch the
 * carrier — and only those. */
static void test_dyn_effect_topology(void) {
    int failed;
    volatile u32 day = 1, t; /* live across TRY (setjmp) */
    fresh_graph(&tg);
    dyn_set_now(day++);
    TRY(run_save("{\"elements\":[{\"name\":\"hot\",\"salience\":0.9},"
                 "{\"name\":\"cold\",\"salience\":0.1}],"
                 "\"facts\":[{\"s\":\"hub\",\"p\":\"uses\",\"o\":\"hot\"},"
                 "{\"s\":\"hub\",\"p\":\"uses\",\"o\":\"cold\"},"
                 "{\"s\":\"hub\",\"p\":\"uses\",\"o\":\"tt\"}]}"), failed);
    CHECK(!failed);
    {
        volatile u32 rel_hot = twr.minted_rels.v[0], rel_cold = twr.minted_rels.v[1];
        volatile u32 e_hot = elem_by_name(&tg, "hot"), e_cold = elem_by_name(&tg, "cold");
        CHECK(tg.elements[e_hot].stats.salience == 0.9);
        /* CPEB (v1 constants): 0.9 > 0.3 threshold -> stability 1 + 1.5*0.9;
         * 0.1 is under the threshold -> untouched */
        CHECK(tg.relations[rel_hot].stats.stability > 2.3499 &&
              tg.relations[rel_hot].stats.stability < 2.3501);
        CHECK(tg.relations[rel_cold].stats.stability == 1.0);
        /* equal-touch decay: five ticks around tt reach hot and cold alike */
        for (t = 0; t < 5; t++) {
            dyn_set_now(day++);
            TRY(run_recall("{\"focus\":[\"tt\"]}"), failed);
            CHECK(!failed);
        }
        CHECK(tg.elements[e_hot].stats.activation > tg.elements[e_cold].stats.activation);
        CHECK(tg.elements[e_cold].stats.activation < 0.1); /* it did decay */
    }
    unsetenv("LEGEND_NOW");
}

/* Exception protection (spec §3.1): a relation whose value carries a digit
 * (dates, quantities) never decays; an undated sibling does; change events
 * (from/to-shaped) are protected as supersession events. */
static void test_dyn_exception_protection(void) {
    int failed;
    u32 rel_dated, rel_plain;
    fresh_graph(&tg);
    dyn_set_now(1);
    TRY(run_save("{\"facts\":[{\"s\":\"beta\",\"p\":\"at\",\"o\":\"August 2026\"},"
                 "{\"s\":\"beta\",\"p\":\"mood\",\"o\":\"gloomy\"},"
                 "{\"s\":\"beta\",\"p\":\"uses\",\"o\":\"other\"}]}"), failed);
    CHECK(!failed);
    rel_dated = twr.minted_rels.v[0];
    rel_plain = twr.minted_rels.v[1];
    CHECK(rel_exception_protected(&tg, rel_dated));
    CHECK(!rel_exception_protected(&tg, rel_plain));
    dyn_set_now(2);
    TRY(run_recall("{\"focus\":[\"other\"]}"), failed); /* decay reaches beta's rels */
    CHECK(!failed);
    CHECK(tg.relations[rel_dated].stats.activation == 0.1);  /* untouched by decay */
    CHECK(tg.relations[rel_plain].stats.activation < 0.1);   /* decayed */
    CHECK(tg.relations[rel_plain].stats.activation > 0.0);
    /* a change event is protected via its from/to shape */
    dyn_set_now(3);
    TRY(run_save("{\"changes\":[{\"target\":\"beta\",\"property\":\"mood\",\"to\":\"sunny\"}]}"),
        failed);
    CHECK(!failed);
    CHECK(rel_exception_protected(&tg, twr.minted_rels.v[0]));
    unsetenv("LEGEND_NOW");
}

/* Determinism (plan §5): the same payload stream under the same LEGEND_NOW
 * values yields byte-identical snapshots and byte-identical frames — with the
 * dynamics live, every float path must reproduce exactly. */
static void test_dyn_determinism(void) {
    int failed;
    static const char *stream[] = {
        "{\"elements\":[{\"name\":\"platformer\",\"kind\":\"project\",\"summary\":\"a game\","
        "\"salience\":0.8}]}",
        "{\"facts\":[{\"s\":\"platformer\",\"p\":\"uses\",\"o\":\"coyote_time\"}],"
        "\"focus\":[\"platformer\"]}",
        "{\"changes\":[{\"target\":\"jump_height\",\"property\":\"value\",\"to\":\"4.2\"}]}",
    };
    static char frame_a[1 << 16];
    volatile u32 i, day; /* live across TRY (setjmp) */
    Hypergraph *gs[2];
    gs[0] = &tg; gs[1] = &tg2;
    for (i = 0; i < 2; i++) {
        day = 1;
        fresh_graph(gs[i]);
        {
            volatile u32 k;
            for (k = 0; k < 3; k++) {
                dyn_set_now(day++);
                TRY(run_save_on(gs[i], stream[k]), failed);
                CHECK(!failed);
            }
        }
        dyn_set_now(day++);
        TRY(run_recall_on(gs[i], "{\"focus\":[\"platformer\"]}"), failed);
        CHECK(!failed);
        {
            u32 len;
            char *buf = capture_frame_json(gs[i], &twr, "/t", 40, 2, -1, &len);
            if (i == 0) {
                CHECK(len < sizeof frame_a);
                memcpy(frame_a, buf, len);
                frame_a[len] = 0;
            } else {
                CHECK(strcmp(frame_a, buf) == 0);
            }
            free(buf);
        }
        snapshot_serialize(gs[i], i == 0 ? &tbb1 : &tbb2);
    }
    CHECK(tbb1.len == tbb2.len && memcmp(tbb1.v, tbb2.v, tbb1.len) == 0);
    unsetenv("LEGEND_NOW");
}

static void test_now_unix_seconds(void) {
    i64 wall;
    int failed;
    setenv("LEGEND_NOW", "1780272000", 1);
    CHECK(now_unix_seconds() == 1780272000);
    setenv("LEGEND_NOW", "17abc", 1); /* malformed override is a hard error, not wall-time */
    TRY((void)now_unix_seconds(), failed);
    CHECK(failed && g_err.code == ERR_PARSE);
    unsetenv("LEGEND_NOW");
    wall = now_unix_seconds();
    CHECK(wall > 1700000000); /* wall clock is sane */
}

/* ---- adjudicated code-review regressions ---- */

/* Finding 1: a merge that overlays two live current_* caches for the same
 * (into, current_<property>) key must leave exactly one live; the loser goes
 * to history with a supersedes meta, not a phantom second state entry. */
static void test_merge_cache_reconcile(void) {
    int failed;
    volatile u32 ca = NONE_U32, cb = NONE_U32; /* live across setjmp */
    u32 beta, curspeed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"changes\":[{\"target\":\"alpha\",\"property\":\"speed\",\"to\":\"10\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 2);
    ca = twr.minted_rels.v[1]; /* alpha's current_speed cache (older) */
    setenv("LEGEND_NOW", "1780358400", 1);
    TRY(run_save("{\"changes\":[{\"target\":\"beta\",\"property\":\"speed\",\"to\":\"20\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 2);
    cb = twr.minted_rels.v[1]; /* beta's current_speed cache (newer) */
    CHECK(tg.relations[ca].status == ST_ASSERTED && tg.relations[cb].status == ST_ASSERTED);

    setenv("LEGEND_NOW", "1780444800", 1);
    TRY(run_save("{\"merge\":[{\"from\":\"alpha\",\"into\":\"beta\"}]}"), failed);
    CHECK(!failed && twr.merge_count == 1);
    beta = elem_by_name(&tg, "beta");
    curspeed = elem_by_name(&tg, "current_speed");
    /* exactly one live cache; the newer wins, the loser is superseded */
    CHECK(tg.relations[cb].status == ST_ASSERTED);
    CHECK(tg.relations[ca].status == ST_SUPERSEDED);
    CHECK(cur_get_live(&tg, beta, curspeed) == cb);
    CHECK(has_rel_meta(&tg, cb, WK_SUPERSEDES, ca));
    /* the loser reads as history, not a second live state entry */
    TRY(run_recall("{\"focus\":[\"beta\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(count_occ(t_frame, "\"current_speed\":\"20\"") == 1); /* live state */
    CHECK(count_occ(t_frame, "\"current_speed\":\"10\"") == 1); /* history */
    CHECK(strstr(t_frame, "superseded_by") != NULL);
    unsetenv("LEGEND_NOW");
}

/* Finding 2: a plain fact and a change on the same (subject, property) pair in
 * one payload — the plain fact (staged before the change, property minted this
 * tick) must be superseded, not left as a second contradictory answer. */
static void test_heal_staged_plain_fact(void) {
    int failed;
    volatile u32 plain = NONE_U32, cache = NONE_U32;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"facts\":[{\"s\":\"X\",\"p\":\"value\",\"o\":\"3.5\"}],"
                 "\"changes\":[{\"target\":\"X\",\"property\":\"value\",\"to\":\"4.2\"}]}"), failed);
    CHECK(!failed && twr.minted_rels.count == 3); /* plain fact, event, cache */
    plain = twr.minted_rels.v[0];
    cache = twr.minted_rels.v[2];
    CHECK(tg.relations[plain].status == ST_SUPERSEDED);
    CHECK(has_rel_meta(&tg, cache, WK_SUPERSEDES, plain));
    {
        u32 xx = elem_by_name(&tg, "X");
        u32 cv = elem_by_name(&tg, "current_value");
        CHECK(cur_get_live(&tg, xx, cv) == cache);
    }
    /* several live values means the property is multi-valued on this subject:
     * one change does not speak for all of them, so none are superseded */
    fresh_graph(&tg);
    TRY(run_save("{\"facts\":[{\"s\":\"rule\",\"p\":\"applies_to\",\"o\":\"editor\"},"
                 "{\"s\":\"rule\",\"p\":\"applies_to\",\"o\":\"sim\"},"
                 "{\"s\":\"rule\",\"p\":\"applies_to\",\"o\":\"view\"}]}"), failed);
    CHECK(!failed);
    TRY(run_save("{\"changes\":[{\"target\":\"rule\",\"property\":\"applies_to\","
                 "\"to\":\"audio\"}]}"), failed);
    CHECK(!failed);
    {
        u32 e = elem_by_name(&tg, "rule");
        u32 ap = elem_by_name(&tg, "applies_to");
        u32 r, live = 0;
        for (r = 0; r < tg.relation_count; r++) {
            const Relation *rel = &tg.relations[r];
            if (rel->status >= ST_SUPERSEDED || rel->attr_count != 2)
                continue;
            if (rel->attrs[0].value.id == e && rel->attrs[1].name == ap)
                live++;
        }
        CHECK(live == 3);
    }
    /* the ontology templates are expects edges the retract guard protects; a
     * change on `expects` must not reach them either, even for a kind whose
     * template lists exactly one (which would otherwise read as single-valued) */
    fresh_graph(&tg);
    TRY(run_save("{\"changes\":[{\"target\":\"question\",\"property\":\"expects\","
                 "\"to\":\"vibes\"}]}"), failed);
    CHECK(!failed);
    {
        u32 r;
        for (r = 0; r < WK_RELATION_COUNT; r++)
            CHECK(tg.relations[r].status < ST_SUPERSEDED);
    }
    unsetenv("LEGEND_NOW");
}

/* Finding 3: a relation a same-payload merge fold drops as Retracted (collision
 * collapse) must not appear in writes.minted_relations. */
static void test_minted_excludes_folded_drop(void) {
    int failed;
    volatile u32 r_from = NONE_U32;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    /* prelude: into_el already carries {uses: shared}; from_el exists too */
    TRY(run_save("{\"elements\":[{\"name\":\"from_el\"}],"
                 "\"facts\":[{\"s\":\"into_el\",\"p\":\"uses\",\"o\":\"shared\"}]}"), failed);
    CHECK(!failed);
    {
        u32 rels_before = tg.relation_count;
        setenv("LEGEND_NOW", "1780358400", 1);
        /* mint a colliding {from_el uses shared}, then fold from_el -> into_el */
        TRY(run_save("{\"facts\":[{\"s\":\"from_el\",\"p\":\"uses\",\"o\":\"shared\"}],"
                     "\"merge\":[{\"from\":\"from_el\",\"into\":\"into_el\"}]}"), failed);
        CHECK(!failed);
        r_from = rels_before; /* the first (only) listed mint this tick */
        CHECK(tg.relations[r_from].status == ST_RETRACTED); /* collision collapse */
        CHECK(!u32vec_has(&twr.minted_rels, r_from));       /* finding 3 filter */
        CHECK(twr.minted_rels.count == 0);
    }
    unsetenv("LEGEND_NOW");
}

/* Finding 4: a closed item (resolved task) leaves its typed section but its
 * still-live relations must not vanish — they belong in recent/related. */
static void test_closed_item_relation_survives(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"elements\":[{\"name\":\"t1\",\"kind\":\"task\","
                 "\"attrs\":{\"description\":\"do the thing\"}}]}"), failed);
    CHECK(!failed);
    setenv("LEGEND_NOW", "1780358400", 1);
    TRY(run_save("{\"elements\":[{\"name\":\"d1\",\"kind\":\"decision\","
                 "\"attrs\":{\"resolves\":\"t1\"}}]}"), failed);
    CHECK(!failed);
    CHECK(elem_is_open(&tg, elem_by_name(&tg, "t1")) == 0); /* closed */
    setenv("LEGEND_NOW", "1780444800", 1);
    TRY(run_recall("{\"focus\":[\"t1\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"open\":[]") != NULL); /* not open */
    CHECK(strstr(t_frame, "\"description\":\"do the thing\"") != NULL); /* survives */
    unsetenv("LEGEND_NOW");
}

/* Finding 5: a template or instance kind whose normalized name equals a builtin
 * frame section key is a parse error at that path. */
static void test_reserved_kind_name(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    expect_save_err("{\"templates\":[{\"kind\":\"recent\",\"expects\":[\"x\"]}]}",
                    ERR_PARSE, "templates[0].kind");
    expect_save_err("{\"templates\":[{\"kind\":\"Recent\",\"expects\":[\"x\"]}]}",
                    ERR_PARSE, "templates[0].kind"); /* normalizes equal */
    expect_save_err("{\"elements\":[{\"name\":\"z\",\"kind\":\"near_matches\"}]}",
                    ERR_PARSE, "elements[0].kind"); /* underscore-form, instance kind */
    TRY(run_save("{\"templates\":[{\"kind\":\"character\",\"expects\":[\"role\"]}]}"), failed);
    CHECK(!failed); /* a non-colliding kind still works */
    TRY(run_save("{\"elements\":[{\"name\":\"z\",\"kind\":\"mechanic\"}]}"), failed);
    CHECK(!failed);
    unsetenv("LEGEND_NOW");
}

/* Finding 6: orientation history must seed only from the displayed (capped)
 * state caches, not the full pre-cap set. */
static void test_orientation_history_cap(void) {
    int failed;
    volatile u32 day = 1, k;
    const char *names[4] = { "aa", "bb", "cc", "dd" };
    fresh_graph(&tg);
    for (k = 0; k < 4; k++) {
        char p[128];
        dyn_set_now(day++);
        snprintf(p, sizeof p, "{\"changes\":[{\"target\":\"%s\",\"property\":\"v\",\"to\":\"1\"}]}", names[k]);
        TRY(run_save(p), failed); CHECK(!failed);
        dyn_set_now(day++);
        snprintf(p, sizeof p, "{\"changes\":[{\"target\":\"%s\",\"property\":\"v\",\"to\":\"2\"}]}", names[k]);
        TRY(run_save(p), failed); CHECK(!failed);
    }
    dyn_set_now(day++);
    TRY(run_recall("{}"), failed); /* orientation: 4 live caches store-wide */
    CHECK(!failed && twr.orientation == 1);
    /* limit 4 -> state capped at 2; history covers only those 2 (finding 6) */
    capture_frame(4, -1, -1);
    CHECK(count_occ(t_frame, "\"current_v\":\"2\"") == 2); /* displayed state */
    CHECK(count_occ(t_frame, "superseded_by") == 2);       /* their chains only */
    unsetenv("LEGEND_NOW");
}

/* Finding 7: retract of a core ontology relation (rel:0..9, the seed expects
 * edges) is a parse error in both the rel-ref and fact-shape forms. */
static void test_retract_core_ontology(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    expect_save_err("{\"retract\":[\"rel:0\"]}", ERR_PARSE, "retract[0]");
    expect_save_err("{\"retract\":[\"rel:9\"]}", ERR_PARSE, "retract[0]");
    /* fact shape: {s:decision, p:expects, o:chose} resolves to rel:0 */
    expect_save_err("{\"retract\":[{\"s\":\"decision\",\"p\":\"expects\",\"o\":\"chose\"}]}",
                    ERR_PARSE, "retract[0]");
    /* a normal retract of a user relation still works */
    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"uses\",\"o\":\"b\"}]}"), failed);
    CHECK(!failed);
    {
        u32 rel = twr.minted_rels.v[0];
        CHECK(rel >= WK_RELATION_COUNT);
        TRY(run_save("{\"retract\":[\"rel:10\"]}"), failed);
        CHECK(!failed && tg.relations[rel].status == ST_RETRACTED);
    }
    unsetenv("LEGEND_NOW");
}

/* Phase 1-3: causal vocabulary, modal annotations, and the recall causal
 * section. caused/correlated_with dedup to one seeded predicate each; a fact's
 * modal reifies a meta-relation; recall surfaces the edge with rung + modal and
 * keeps it out of recent/related. */
static void test_causal(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    /* seeded predicates dedup: two `caused` facts share one predicate element */
    TRY(run_save("{\"facts\":["
                 "{\"s\":\"deploy\",\"p\":\"caused\",\"o\":\"outage\",\"modal\":[\"intervened\"]},"
                 "{\"s\":\"migration\",\"p\":\"prevents\",\"o\":\"outage\",\"modal\":[\"non_actual\"]},"
                 "{\"s\":\"spike\",\"p\":\"correlated_with\",\"o\":\"outage\"}]}"), failed);
    CHECK(!failed);
    {
        u32 nlen = normalize_into_scratch("caused", 6);
        u32 caused = resolve_tier1(&tg, g_norm_buf, nlen, NONE_U32);
        CHECK(caused == tg.wk_ext[EXT_CAUSED] && caused != NONE_U32);
    }
    /* recall the effect: the causal section carries rung + modal, and the edges
     * do not also appear in recent/related */
    TRY(run_recall("{\"focus\":[\"outage\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"causal\":[") != NULL);
    CHECK(strstr(t_frame, "\"caused\":\"outage\"") != NULL);
    CHECK(strstr(t_frame, "\"rung\":\"causal\"") != NULL);
    CHECK(strstr(t_frame, "\"rung\":\"correlational\"") != NULL);
    CHECK(strstr(t_frame, "\"modal\":[\"intervened\"]") != NULL);
    CHECK(strstr(t_frame, "\"modal\":[\"non_actual\"]") != NULL);
    CHECK(strstr(t_frame, "\"recent\":[]") != NULL);
    CHECK(strstr(t_frame, "\"related\":[]") != NULL);
    /* #616 regression: a modal on a NON-causal predicate must still surface at
     * recall -- it renders on the recent/related entry, not only in the causal
     * section -- or a negated fact reads as a plain asserted one (an inverted
     * claim). A plain fact carries no modal key (empty "modal":[] never emits). */
    TRY(run_save("{\"facts\":["
                 "{\"s\":\"readability\",\"p\":\"justifies\",\"o\":\"text assets\",\"modal\":[\"negated\"]},"
                 "{\"s\":\"cat\",\"p\":\"sat on\",\"o\":\"mat\"}]}"), failed);
    CHECK(!failed);
    TRY(run_recall("{\"focus\":[\"text assets\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"justifies\":\"text assets\"") != NULL);
    CHECK(strstr(t_frame, "\"modal\":[\"negated\"]") != NULL);
    CHECK(strstr(t_frame, "\"modal\":[]") == NULL);
    TRY(run_recall("{\"focus\":[\"mat\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"sat on\":\"mat\"") != NULL);
    CHECK(strstr(t_frame, "\"modal\"") == NULL);
    /* an unknown modal is a parse error, and a causal predicate is protected */
    TRY(run_save("{\"facts\":[{\"s\":\"a\",\"p\":\"caused\",\"o\":\"b\",\"modal\":[\"bogus\"]}]}"), failed);
    CHECK(failed);
    TRY(run_save("{\"merge\":[{\"from\":\"caused\",\"into\":\"prevents\"}]}"), failed);
    CHECK(failed);
    unsetenv("LEGEND_NOW");
}

static void test_frame_section_caps(void) {
    int failed;
    u32 i;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    /* 13 decisions: the typed sections used to emit every one of them, which
     * is how the live trial packet reached 51KB against a 4000-byte hook cap
     * and starved the model of the very sections it boots to read */
    for (i = 0; i < 13; i++) {
        char buf[256];
        snprintf(buf, sizeof buf,
                 "{\"elements\":[{\"name\":\"choice %u\",\"kind\":\"decision\","
                 "\"summary\":\"one of many\"}]}", i);
        TRY(run_save(buf), failed);
        CHECK(!failed);
    }
    TRY(run_recall("{}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    /* newest-first, so 12..3 make the cut and 2..0 are held back. Match the
     * closing quote: "choice 1" is a prefix of "choice 12". */
    CHECK(strstr(t_frame, "\"name\":\"choice 3\"") != NULL);
    CHECK(strstr(t_frame, "\"name\":\"choice 2\"") == NULL);
    CHECK(strstr(t_frame, "\"name\":\"choice 0\"") == NULL);
    CHECK(strstr(t_frame, "\"omitted\":{\"decisions\":3}") != NULL);
    /* nothing was pruned -- the store still holds all 13, and a focused
     * recall still reaches one the cap held back */
    CHECK(tg.element_count > 13);
    TRY(run_recall("{\"focus\":[\"choice 0\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "choice 0") != NULL);
    /* a store under the cap says nothing about omission */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"only choice\",\"kind\":\"decision\","
                 "\"summary\":\"the one\"}]}"),
        failed);
    CHECK(!failed);
    TRY(run_recall("{}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "only choice") != NULL);
    CHECK(strstr(t_frame, "omitted") == NULL);
    unsetenv("LEGEND_NOW");
}

static char t_audit[1 << 16];

static void capture_audit_limit(i64 per_reason) {
    FILE *tmp = tmpfile();
    int saved;
    long n;
    CHECK(tmp != NULL);
    if (!tmp) { t_audit[0] = 0; return; }
    fflush(stdout);
    saved = dup(1);
    CHECK(saved >= 0 && dup2(fileno(tmp), 1) >= 0);
    audit_graph(&tg, per_reason);
    fflush(stdout);
    dup2(saved, 1);
    close(saved);
    n = ftell(tmp);
    if (n < 0) n = 0;
    if ((size_t)n >= sizeof t_audit) n = (long)sizeof t_audit - 1;
    rewind(tmp);
    if (n > 0 && fread(t_audit, 1, (size_t)n, tmp) != (size_t)n) n = 0;
    t_audit[n] = 0;
    fclose(tmp);
}

static void capture_audit(void) { capture_audit_limit((i64)g_aud_per_reason); }

/* The two defect classes round 8 found BY HAND after they sat live and
 * invisible to every audit check. Neither can be produced through the save path
 * any more (e6973ae rejects a claim in the kind slot, f76d3e6 rejects a plain
 * current_* write), so the state is built directly -- a check that reads 0 on a
 * clean store proves nothing about whether it can fire. */
static void test_audit_finds_round8_defects(void) {
    u32 subj, bogus, standing, active, settled;
    Attr at[2];
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    TRY(run_save("{\"elements\":[{\"name\":\"Bio Weapon\",\"kind\":\"spell\","
                 "\"summary\":\"a poison mask\"}]}"), failed);
    CHECK(!failed);
    subj = twr.minted_elems.v[0];
    CHECK(elem_name_is(&tg, subj, "Bio Weapon"));
    capture_audit();
    CHECK(strstr(t_audit, "clobbered_kind\":0") != NULL); /* clean to start */

    /* a SECOND instance_of carrying a claim -- exactly #574's shape: both edges
     * live, elem_kind picks the wrong one, and the element silently drops out of
     * every kind-keyed check */
    bogus = mint_element(&tg, "nothing resolves on cast", 24, 1.0, 0.0, tg.clock);
    at[0].name = WK_SUBJECT; at[0].value.tag = TERM_ELEM; at[0].value.id = subj;
    at[1].name = WK_INSTANCE_OF; at[1].value.tag = TERM_ELEM; at[1].value.id = bogus;
    mint_relation(&tg, at, 2, ST_ASSERTED, 1.0, 0.0, tg.clock, NONE_U32);
    capture_audit();
    CHECK(strstr(t_audit, "\"clobbered_kind\":1") != NULL);
    CHECK(strstr(t_audit, "nothing resolves on cast") != NULL);

    /* two LIVE current_standing caches on one subject -- #602's shape. The cur
     * index holds one entry per pair, so the second is invisible there while
     * both sit in the graph and recall can surface either. */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"the brake count\",\"kind\":\"question\","
                 "\"summary\":\"how many limiters\"}]}"), failed);
    CHECK(!failed);
    subj = twr.minted_elems.v[0];
    standing = mint_element(&tg, "current_standing", 16, 1.0, 0.0, tg.clock);
    active = mint_element(&tg, "active", 6, 1.0, 0.0, tg.clock);
    settled = mint_element(&tg, "settled", 7, 1.0, 0.0, tg.clock);
    at[0].name = WK_SUBJECT; at[0].value.tag = TERM_ELEM; at[0].value.id = subj;
    at[1].name = standing; at[1].value.tag = TERM_ELEM; at[1].value.id = active;
    mint_relation(&tg, at, 2, ST_ASSERTED, 1.0, 0.0, tg.clock, NONE_U32);
    capture_audit();
    CHECK(strstr(t_audit, "\"dup_cache\":0") != NULL); /* one cache is correct */
    at[1].value.id = settled;
    mint_relation(&tg, at, 2, ST_ASSERTED, 1.0, 0.0, tg.clock, NONE_U32);
    capture_audit();
    CHECK(strstr(t_audit, "\"dup_cache\":1") != NULL); /* the second is the defect */

    /* a SUPERSEDED second cache is the normal history shape, not a defect */
    tg.relations[tg.relation_count - 1].status = ST_SUPERSEDED;
    capture_audit();
    CHECK(strstr(t_audit, "\"dup_cache\":0") != NULL);
}

/* Rates ship beside counts, because a raw count rises with the store and misled
 * everyone who read this output -- including its authors -- for two rounds. */
static void test_audit_rates(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);
    TRY(run_save("{\"elements\":[{\"name\":\"a\",\"kind\":\"system\",\"summary\":\"x\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"per_1k_elements\":{") != NULL);
    CHECK(strstr(t_audit, "\"bloat\":0.0") != NULL);
}

/* Status-like values belong in the cache the `changes` verb owns. The guard
 * used to require kind==constraint AND a new mint AND the literal predicate
 * `standing`; the audit flagged every case, so the save path kept creating a
 * defect the audit then reported. These are the three paths that were live on
 * the trial store. */
static void test_status_value_paths(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    /* (b) a NON-constraint mint: kind is irrelevant, what matters is that the
     * value changes */
    TRY(run_save("{\"elements\":[{\"name\":\"the inverse clause\",\"kind\":\"decision\","
                 "\"summary\":\"s\",\"attrs\":{\"standing\":\"active\"}}]}"), failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"status_fact\":0") != NULL);

    /* (c) a status-flavored predicate that is not `standing` */
    TRY(run_save("{\"elements\":[{\"name\":\"the Lightning branch\",\"kind\":\"system\","
                 "\"summary\":\"s\",\"attrs\":{\"status\":\"locked\"}}]}"), failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"status_fact\":0") != NULL);

    /* the value must be SEEDED, not dropped: skipping the plain fact without
     * creating the cache would lose it outright */
    TRY(run_recall("{\"focus\":[\"the Lightning branch\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "current_status") != NULL);
    CHECK(strstr(t_frame, "locked") != NULL);

    /* (a) the same attr on an element that ALREADY EXISTS is refused: it has a
     * current value to supersede, and seeding a second live cache beside it is
     * the dup_cache defect */
    TRY(run_save("{\"elements\":[{\"name\":\"the inverse clause\",\"summary\":\"s2\","
                 "\"attrs\":{\"standing\":\"retired\"}}]}"), failed);
    CHECK(failed && g_err.code == ERR_PARSE);

    /* ...and the path it names works, superseding rather than accreting */
    TRY(run_save("{\"changes\":[{\"target\":\"the inverse clause\","
                 "\"property\":\"standing\",\"to\":\"retired\"}]}"), failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"status_fact\":0") != NULL);
    CHECK(strstr(t_audit, "\"dup_cache\":0") != NULL);

    /* a constraint mint still seeds `active` with no attr supplied at all */
    TRY(run_save("{\"elements\":[{\"name\":\"no bespoke counters\","
                 "\"kind\":\"constraint\",\"summary\":\"s\"}]}"), failed);
    CHECK(!failed);
    TRY(run_recall("{\"focus\":[\"no bespoke counters\"]}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "current_standing") != NULL);
}

static void test_audit(void) {
    int failed;
    fresh_graph(&tg);
    setenv("LEGEND_NOW", "1780272000", 1);

    /* a well-formed store is silent: every check must earn its output, or a
     * maintenance pass trains the human to skim past it */
    TRY(run_save("{\"elements\":["
                 "{\"name\":\"jump feel\",\"kind\":\"system\",\"summary\":\"how jumping reads\"},"
                 "{\"name\":\"coyote time\",\"kind\":\"parameter\",\"summary\":\"grace window\"}],"
                 "\"facts\":[{\"s\":\"jump feel\",\"p\":\"uses\",\"o\":\"coyote time\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"suspects\":[]") != NULL);
    CHECK(strstr(t_audit, "\"total\":0") != NULL);
    CHECK(strstr(t_audit, "\"truncated\":{}") != NULL);

    /* audit never writes: same clock before and after, so it is safe to run
     * against a live store mid-session */
    {
        u32 before = tg.clock;
        capture_audit();
        CHECK(tg.clock == before);
    }

    /* resolves.o must_exist (Phase 1): a resolves whose target nobody opened is
     * now REJECTED at the source -- the finding-6 papercut is PREVENTED, not just
     * flagged by the audit after the phantom lands. */
    TRY(run_save("{\"elements\":[{\"name\":\"real question\",\"kind\":\"question\","
                 "\"summary\":\"genuinely open\"},"
                 "{\"name\":\"the fix\",\"kind\":\"decision\",\"summary\":\"what landed\"}]}"),
        failed);
    CHECK(!failed);
    TRY(run_save("{\"facts\":[{\"s\":\"the fix\",\"p\":\"resolves\","
                 "\"o\":\"a question nobody opened\"}]}"),
        failed);
    CHECK(failed); /* rejected: the resolves target must already exist */
    capture_audit();
    CHECK(strstr(t_audit, "a question nobody opened") == NULL); /* no phantom minted */
    /* resolving a REAL open item is correct usage and succeeds */
    TRY(run_save("{\"facts\":[{\"s\":\"the fix\",\"p\":\"resolves\",\"o\":\"real question\"}]}"),
        failed);
    CHECK(!failed);

    /* status_fact: a status-flavored property written as a plain fact
     * accretes instead of superseding (trial doc §11) */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"phase 0 build\",\"kind\":\"task\",\"summary\":\"the build\"}],"
                 "\"facts\":[{\"s\":\"phase 0 build\",\"p\":\"status\",\"o\":\"M0 green\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"reason\":\"status_fact\"") != NULL);
    CHECK(strstr(t_audit, "\"status\":\"M0 green\"") != NULL);
    /* the same value through `changes` is the right shape and is not flagged */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"phase 0 build\",\"kind\":\"task\",\"summary\":\"the build\"}],"
                 "\"changes\":[{\"target\":\"phase 0 build\",\"property\":\"status\",\"to\":\"M0 green\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"status_fact\":0") != NULL);

    /* flat_decision: a decision whose choice lives only in its name + summary
     * -- no chose/rejected/about/resolves -- is flagged, because the entities
     * it decides over ("Bolt", "Melee") never became nodes. */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"Bolt as the default\",\"kind\":\"decision\","
                 "\"summary\":\"a dropped shape note yields Bolt, not Melee\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"reason\":\"flat_decision\"") != NULL);
    CHECK(strstr(t_audit, "\"name\":\"Bolt as the default\"") != NULL);
    /* the same decision as a hub -- named for the topic, the choice broken out
     * into chose/rejected -- is the right shape and clears the flag (and this
     * is what mints Bolt + Melee as real, linkable nodes) */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"default fumble shape\",\"kind\":\"decision\","
                 "\"summary\":\"what a dropped shape note yields\","
                 "\"attrs\":{\"chose\":\"Bolt\",\"rejected\":\"Melee\","
                 "\"reason\":\"Bolt is the neutral option\"}}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"flat_decision\":0") != NULL);

    /* an option a decision hub broke out is NOT itself a flat decision, even
     * when the model mis-kinds it `decision` (the C/Rust/SDL case): it is
     * referenced as a chose/rejected value, so it is a target, not a flat hub. */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":["
                 "{\"name\":\"implementation language\",\"kind\":\"decision\","
                 "\"attrs\":{\"chose\":\"C\",\"rejected\":\"Rust\"}},"
                 "{\"name\":\"C\",\"kind\":\"decision\",\"summary\":\"the language\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"flat_decision\":0") != NULL);

    /* near_dup: catches a typo twin, and the digit guard keeps numbered
     * siblings apart -- "round 1" vs "round 2" scores HIGHER on trigrams
     * (0.83) than the real duplicate does (0.63), so similarity alone
     * cannot separate them and a differing digit has to veto the pair */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":["
                 "{\"name\":\"ambient recall abstention\",\"kind\":\"task\",\"summary\":\"a\"},"
                 "{\"name\":\"ambient recall abstension\",\"kind\":\"task\",\"summary\":\"b\"},"
                 "{\"name\":\"trial round 1\",\"kind\":\"event\",\"summary\":\"c\"},"
                 "{\"name\":\"trial round 2\",\"kind\":\"event\",\"summary\":\"d\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"near_dup\":1") != NULL);
    CHECK(strstr(t_audit, "\"other_name\":\"ambient recall abstention\"") != NULL);
    CHECK(strstr(t_audit, "trial round") == NULL);
    /* the threshold sits above same-topic/different-subsystem names, which are
     * the false positives the trial store actually produced (0.685) */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":["
                 "{\"name\":\"regions design adversarial review\",\"kind\":\"event\",\"summary\":\"a\"},"
                 "{\"name\":\"loot design adversarial review\",\"kind\":\"event\",\"summary\":\"b\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"near_dup\":0") != NULL);
    /* a `changes` from/to pair is supersession history, not duplication: the
     * old and new values of an EDITED string are near-identical by nature,
     * and folding them would destroy the history `changes` exists to keep */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"spell drops\",\"kind\":\"system\","
                 "\"summary\":\"the drop table\"}],"
                 "\"changes\":[{\"target\":\"spell drops\",\"property\":\"rarity_vocabulary\","
                 "\"from\":\"tier is rarity: Common/Rare/Epic/Mythic\","
                 "\"to\":\"tier is rarity: Common/Rare/Fabled/Mythic\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"near_dup\":0") != NULL);

    /* nor is a child named after its parent: splitting an overgrown summary
     * into a short core plus children is the documented remedy for bloat, so
     * taking the tool's own advice must not manufacture suspects here */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":["
                 "{\"name\":\"color-signature summons\",\"kind\":\"decision\",\"summary\":\"the core\"},"
                 "{\"name\":\"color-signature summons roster\",\"kind\":\"system\",\"summary\":\"the detail\"}],"
                 "\"facts\":[{\"s\":\"color-signature summons roster\",\"p\":\"part_of\","
                 "\"o\":\"color-signature summons\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"near_dup\":0") != NULL);

    /* short names are incomparable by trigram: one differing byte out of six
     * still leaves ~0.6, so "elem a"/"elem b" would pair every sibling with
     * every other. Below the floor, no pairing at all. */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":["
                 "{\"name\":\"elem a\",\"kind\":\"task\",\"summary\":\"a\"},"
                 "{\"name\":\"elem b\",\"kind\":\"task\",\"summary\":\"b\"},"
                 "{\"name\":\"elem c\",\"kind\":\"task\",\"summary\":\"c\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"near_dup\":0") != NULL);

    /* prose_name: a whole sentence passed where a canonical name belongs */
    fresh_graph(&tg);
    TRY(run_save("{\"facts\":[{\"s\":\"lesson\",\"p\":\"is\",\"o\":\"measurements need "
                 "provenance: session-4 self-play stats were saved without how they "
                 "were run, forcing a from-scratch arena rebuild in session 5\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"reason\":\"prose_name\"") != NULL);
    CHECK(strstr(t_audit, "\"name_chars\":143") != NULL);
    /* the short subject of that same fact is not prose and is not flagged */
    CHECK(strstr(t_audit, "\"prose_name\":1") != NULL);

    /* stale_open: an open item nobody has touched in a long time. Driven from
     * a small store by shrinking the threshold rather than burning 50 ticks --
     * only open KINDS count, and a `resolves` edge takes an item off the list
     * however old it is (rewriting its summary would not). */
    fresh_graph(&tg);
    {
        u32 saved_ticks = g_aud_stale_ticks;
        TRY(run_save("{\"elements\":["
                     "{\"name\":\"forgotten question\",\"kind\":\"question\",\"summary\":\"never answered\"},"
                     "{\"name\":\"answered question\",\"kind\":\"question\",\"summary\":\"since closed\"},"
                     "{\"name\":\"settled decision\",\"kind\":\"decision\",\"summary\":\"not an open kind\"}]}"),
            failed);
        CHECK(!failed);
        TRY(run_save("{\"facts\":[{\"s\":\"settled decision\",\"p\":\"resolves\","
                     "\"o\":\"answered question\"}]}"),
            failed);
        CHECK(!failed);
        /* age the closed question PAST the threshold too, so the `resolves`
         * edge is the only thing keeping it off the list. Without this it
         * stays silent merely by being recent, and the exemption goes
         * untested (removing the resolves check would not fail the test). */
        TRY(run_save("{\"elements\":[{\"name\":\"filler one\",\"kind\":\"event\","
                     "\"summary\":\"advances the clock\"}]}"),
            failed);
        CHECK(!failed);
        TRY(run_save("{\"elements\":[{\"name\":\"later work\",\"kind\":\"task\","
                     "\"summary\":\"touched just now\"}]}"),
            failed);
        CHECK(!failed);
        g_aud_stale_ticks = 1;
        capture_audit();
        g_aud_stale_ticks = saved_ticks;
        CHECK(strstr(t_audit, "\"reason\":\"stale_open\"") != NULL);
        CHECK(strstr(t_audit, "\"name\":\"forgotten question\"") != NULL);
        CHECK(strstr(t_audit, "\"stale_open\":1") != NULL); /* not the resolved
                                                             one, not the
                                                             decision, not the
                                                             fresh task */
        CHECK(strstr(t_audit, "\"silent_for\":") != NULL);
    }

    /* orphan: what a retraction leaves behind. The retracted fact's object
     * keeps its element but nothing live points at it any more -- inert, so
     * it ranks below the defects rather than beside them. The subject carries
     * a summary and is therefore never a candidate. */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"described thing\",\"kind\":\"system\","
                 "\"summary\":\"has a summary, never orphans\"}],"
                 "\"facts\":[{\"s\":\"described thing\",\"p\":\"mentions\",\"o\":\"dangling name\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"orphan\":0") != NULL); /* still referenced: silent */
    TRY(run_save("{\"retract\":[{\"s\":\"described thing\",\"p\":\"mentions\","
                 "\"o\":\"dangling name\"}]}"),
        failed);
    CHECK(!failed);
    capture_audit();
    CHECK(strstr(t_audit, "\"reason\":\"orphan\"") != NULL);
    CHECK(strstr(t_audit, "\"name\":\"dangling name\"") != NULL);
    CHECK(strstr(t_audit, "\"orphan\":1") != NULL);
    CHECK(strstr(t_audit, "described thing") == NULL);

    /* the per-reason cap keeps the list readable, and says what it dropped:
     * six bloated summaries, five shown, one named in `truncated` */
    fresh_graph(&tg);
    {
        static char big[4096];
        u32 i;
        char *p = big;
        p += sprintf(p, "{\"elements\":[");
        for (i = 0; i < 6; i++) {
            u32 k;
            p += sprintf(p, "%s{\"name\":\"elem %c\",\"kind\":\"task\",\"summary\":\"",
                         i ? "," : "", (char)('a' + i));
            for (k = 0; k < 45; k++) p += sprintf(p, "ten chars ");
            p += sprintf(p, "\"}");
        }
        sprintf(p, "]}");
        TRY(run_save(big), failed);
        CHECK(!failed);
    }
    capture_audit();
    CHECK(strstr(t_audit, "\"bloat\":6") != NULL);
    CHECK(strstr(t_audit, "\"truncated\":{\"bloat\":1}") != NULL);
    CHECK(strstr(t_audit, "\"shown\":5") != NULL);
    CHECK(strstr(t_audit, "\"total\":6") != NULL);
    /* ...and `limit` lifts it, which is what a maintenance pass needs: five
     * at a time is not triage when a group holds seventeen */
    capture_audit_limit(-1);
    CHECK(strstr(t_audit, "\"shown\":6") != NULL);
    CHECK(strstr(t_audit, "\"truncated\":{}") != NULL);
    capture_audit_limit(2);
    CHECK(strstr(t_audit, "\"shown\":2") != NULL);
    CHECK(strstr(t_audit, "\"truncated\":{\"bloat\":4}") != NULL);

    /* the orientation tally: same checks, in the packet header where the
     * SessionStart hook's `head -c 4000` cannot cut it off, and WITHOUT the
     * O(n^2) pair sweep that would slow every session start */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":["
                 "{\"name\":\"phase 0 build\",\"kind\":\"task\",\"summary\":\"the build\"},"
                 "{\"name\":\"ambient recall abstention\",\"kind\":\"task\",\"summary\":\"a\"},"
                 "{\"name\":\"ambient recall abstension\",\"kind\":\"task\",\"summary\":\"b\"}],"
                 "\"facts\":[{\"s\":\"phase 0 build\",\"p\":\"status\",\"o\":\"M0 green\"}]}"),
        failed);
    CHECK(!failed);
    {
        u32 saved_bloat = g_aud_bloat_chars;
        g_aud_bloat_chars = 5; /* make "the build" (9ch) count as bloat */
        capture_audit();
        CHECK(strstr(t_audit, "\"near_dup\":1") != NULL); /* the full scan sees it */
        CHECK(strstr(t_audit, "\"bloat\":") != NULL);     /* the verb reports it */
        TRY(run_recall("{}"), failed);
        CHECK(!failed);
        capture_frame(40, 2, -1);
        g_aud_bloat_chars = saved_bloat;
    }
    CHECK(strstr(t_frame, "\"audit\":{") != NULL);
    CHECK(strstr(t_frame, "\"status_fact\":1") != NULL);
    CHECK(strstr(t_frame, "near_dup") == NULL); /* ...the tally never does */
    CHECK(strstr(t_frame, "bloat") == NULL);    /* ...nor bloat: it climbs with
                                                   store size, so it lives in
                                                   `legend audit`, not here (#122) */
    /* it sits in the overview header, ahead of scope/active */
    {
        const char *ov = strstr(t_frame, "\"overview\":{");
        const char *aud = strstr(t_frame, "\"audit\":{");
        const char *scope = strstr(t_frame, "\"scope\":");
        CHECK(ov != NULL && aud != NULL && scope != NULL);
        if (ov && aud && scope) CHECK(ov < aud && aud < scope);
    }
    /* a clean store says nothing at all: maintenance is pull, not a nag */
    fresh_graph(&tg);
    TRY(run_save("{\"elements\":[{\"name\":\"jump feel\",\"kind\":\"system\","
                 "\"summary\":\"how jumping reads\"}]}"),
        failed);
    CHECK(!failed);
    TRY(run_recall("{}"), failed);
    CHECK(!failed);
    capture_frame(40, 2, -1);
    CHECK(strstr(t_frame, "\"overview\":{") != NULL);
    CHECK(strstr(t_frame, "\"audit\"") == NULL);

    unsetenv("LEGEND_NOW");
}

int main(void) {
    test_normalize();
    test_trigrams();
    test_str_arena();
    test_str_map();
    test_tokenizer();
    test_utf8_payload_door();
    test_reader_spec_example();
    test_reader_accepts();
    test_reader_rejections();
    test_reader_recall();
    test_payload_cap();
    test_journal_last_build();
    test_store_discovery();
    test_flock_conflict();
    test_now_unix_seconds();
    test_number_formatter();
    test_iso_format();
    test_ontology_ids();
    test_self_anchor();
    test_tier1_resolution();
    test_kind_change();
    test_relation_dedup();
    test_write_report_shape();
    test_supersession_chain();
    test_prose_backstop();
    test_event_fact_equivalence();
    test_retract();
    test_merge_fold();
    test_rename_and_visibility();
    test_template_drift();
    test_promotion();
    test_salience_seeds();
    test_element_src();
    test_constraint_cache();
    test_recall_tick();
    test_summary_cap();
    test_nested_statement();
    test_tier2_read();
    test_near_matches();
    test_section_filters();
    test_history_since_limit();
    test_related_ranking();
    test_orientation_frame();
    test_observe_identity();
    test_custom_kind_section();
    test_dyn_spaced_vs_massed();
    test_dyn_effect_topology();
    test_dyn_exception_protection();
    test_dyn_determinism();
    test_store_full();
    test_persistence_roundtrip();
    test_snapshot_corrupt();
    test_merge_cache_reconcile();
    test_heal_staged_plain_fact();
    test_minted_excludes_folded_drop();
    test_closed_item_relation_survives();
    test_reserved_kind_name();
    test_orientation_history_cap();
    test_retract_core_ontology();
    test_causal();
    test_frame_section_caps();
    test_audit();
    test_status_value_paths();
    test_audit_finds_round8_defects();
    test_audit_rates();

    json_free(&tj);
    sub_free(&tsub);
    recall_free(&trec);
    recall_free(&t_rec);
    graph_free(&tg);
    graph_free(&tg2);
    report_reset(&twr);
    free(tbb1.v);
    free(tbb2.v);

    printf("legend_test: %d checks, %d failures\n", t_checks, t_fails);
    return t_fails ? 1 : 0;
}
