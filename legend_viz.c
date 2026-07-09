/* legend_viz.c — native X11 hypergraph viewer for a Legend store.
 *
 * Build:  cc -std=c99 -Wall -Wextra -Werror -O2 legend_viz.c embed.c \
 *             -o legend-viz -lX11 -lm
 * Run:    ./legend-viz <store-dir | legend.snapshot file>
 *         ./legend-viz              store discovery like the CLI
 *                                   (LEGEND_STATE_DIR, else nearest .legend)
 *         ./legend-viz ... --check  headless: load, layout, print counts, exit
 *
 * Rendering model (the set-system projection of the hypergraph): every
 * ELEMENT is a circle, colored by kind; every RELATION is a closed boundary
 * drawn AROUND its member circles — the slot VALUES. Slot NAMES are labels
 * (shown in the panel), never members: "subject" as a member would enclose
 * the whole store. A meta relation (a slot value that is another relation)
 * encloses that relation's handle point, a small square at its centroid.
 *
 * Controls: left-drag / arrows pan · wheel or +/- zoom · click = select
 * (element circle first, else the smallest boundary under the cursor) ·
 * panel rows are clickable and jump the selection · j/k scroll the panel ·
 * m meta relations · d dead (superseded/retracted) · v seed vocab ·
 * l labels · r reshake layout · Esc deselect · q quit.
 */

#define LEGEND_NO_MAIN
#include "legend.c"

#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/keysym.h>

enum { PANEL_W = 380, WIN_W = 1280, WIN_H = 800 };
enum { HULL_SAMPLES = 8, MAX_HULL = 5 * (HULL_SAMPLES + 1) + 8 };
enum { PANEL_ROWS = 512 };

typedef struct {
  double x, y, vx, vy;
  double r;      /* draw radius (world units) */
  int visible;
  int degree;
} VElem;

typedef struct {
  u32 members[10];  /* element ids or (rel-handle marker | 0x80000000) */
  u32 member_count;
  double hx, hy;    /* handle: centroid of members */
  double area;      /* hull area, for smallest-wins picking */
  int visible;
  XPoint hull[MAX_HULL];
  int hull_n;
} VRel;

static VElem *ve;
static VRel *vr;
static u32 ne, nr;

/* selection: kind 0 = none, 1 = element, 2 = relation */
static int sel_kind;
static u32 sel_id;

/* view transform: screen = (world - cam) * zoom + center */
static double cam_x, cam_y, zoom = 1.0;
static int show_meta, show_dead, show_vocab, show_labels = 1;
static int panel_scroll;

/* panel click targets: row -> what it selects */
typedef struct {
  int y0, y1;
  int kind; /* 1 elem, 2 rel */
  u32 id;
} PanelRow;
static PanelRow prows[PANEL_ROWS];
static int prow_count;

/* ---- deterministic pseudo-random from ids (no rand(): reshake-stable) ---- */
static double id_hash01(u32 id, u32 salt) {
  u32 h = fnv1a((const char *)&id, 4) ^ (salt * 2654435761u);
  return (double)(h & 0xFFFFFF) / (double)0xFFFFFF;
}

/* ---- visibility ---- */

static int rel_is_dead(u32 rid) {
  return g_graph.relations[rid].status >= ST_SUPERSEDED;
}

static void recompute_visibility(void) {
  u32 i, k;
  for (i = 0; i < ne; i++) {
    ve[i].visible = g_graph.elements[i].redirect == NONE_U32 &&
                    (show_vocab || i >= WK_ELEMENT_COUNT);
    ve[i].degree = 0;
  }
  for (i = 0; i < nr; i++) {
    const Relation *r = &g_graph.relations[i];
    int vis = (show_dead || !rel_is_dead(i)) &&
              (show_meta || !rel_is_meta(&g_graph, i));
    if (i < WK_RELATION_COUNT && !show_vocab)
      vis = 0; /* seed expects-templates live with the vocab toggle */
    /* a relation is only drawable if every member is visible */
    for (k = 0; k < r->attr_count && vis; k++) {
      const Term *t = &r->attrs[k].value;
      if (t->tag == TERM_ELEM && !ve[t->id].visible)
        vis = 0;
      if (t->tag == TERM_REL &&
          (!show_meta || (!show_dead && rel_is_dead(t->id))))
        vis = 0;
    }
    vr[i].visible = vis;
  }
  /* degree drives element radius: count visible relation memberships */
  for (i = 0; i < nr; i++) {
    const Relation *r = &g_graph.relations[i];
    if (!vr[i].visible)
      continue;
    for (k = 0; k < r->attr_count; k++)
      if (r->attrs[k].value.tag == TERM_ELEM)
        ve[r->attrs[k].value.id].degree++;
  }
  for (i = 0; i < ne; i++)
    ve[i].r = 6.0 + 2.5 * sqrt((double)ve[i].degree);
}

static void collect_members(u32 rid) {
  const Relation *r = &g_graph.relations[rid];
  u32 k;
  vr[rid].member_count = 0;
  for (k = 0; k < r->attr_count; k++) {
    const Term *t = &r->attrs[k].value;
    u32 m = t->tag == TERM_ELEM ? t->id : (t->id | 0x80000000u);
    u32 j, dup = 0;
    for (j = 0; j < vr[rid].member_count; j++)
      if (vr[rid].members[j] == m)
        dup = 1;
    if (!dup && vr[rid].member_count < 10)
      vr[rid].members[vr[rid].member_count++] = m;
  }
}

static void member_pos(u32 m, double *x, double *y, double *rad) {
  if (m & 0x80000000u) {
    const VRel *q = &vr[m & 0x7FFFFFFFu];
    *x = q->hx;
    *y = q->hy;
    *rad = 6.0;
  } else {
    *x = ve[m].x;
    *y = ve[m].y;
    *rad = ve[m].r;
  }
}

/* ---- force layout ----
 * elements repel (O(n^2), fine at trial scale); each visible relation pulls
 * its members toward its handle; weak gravity re-centers. Run to a fixed
 * iteration budget on load / reshake, then a few steps per frame to settle. */

static void layout_seed(void) {
  u32 i;
  double R = 60.0 * sqrt((double)ne);
  for (i = 0; i < ne; i++) {
    double a = id_hash01(i, 1) * 6.283185307, d = sqrt(id_hash01(i, 2)) * R;
    ve[i].x = cos(a) * d;
    ve[i].y = sin(a) * d;
    ve[i].vx = ve[i].vy = 0;
  }
}

static void layout_step(void) {
  u32 i, j, k;
  for (i = 0; i < nr; i++) { /* handles first: springs pull toward them */
    double sx = 0, sy = 0;
    if (!vr[i].visible || vr[i].member_count == 0)
      continue;
    for (k = 0; k < vr[i].member_count; k++) {
      double x, y, r0;
      member_pos(vr[i].members[k], &x, &y, &r0);
      sx += x;
      sy += y;
    }
    vr[i].hx = sx / vr[i].member_count;
    vr[i].hy = sy / vr[i].member_count;
  }
  for (i = 0; i < ne; i++) {
    double fx = 0, fy = 0;
    if (!ve[i].visible)
      continue;
    for (j = 0; j < ne; j++) {
      double dx, dy, d2, f;
      if (j == i || !ve[j].visible)
        continue;
      dx = ve[i].x - ve[j].x;
      dy = ve[i].y - ve[j].y;
      d2 = dx * dx + dy * dy + 40.0;
      f = 5200.0 / d2;
      fx += dx * f;
      fy += dy * f;
    }
    fx -= ve[i].x * 0.012; /* gravity */
    fy -= ve[i].y * 0.012;
    ve[i].vx = (ve[i].vx + fx) * 0.62;
    ve[i].vy = (ve[i].vy + fy) * 0.62;
  }
  for (i = 0; i < nr; i++) { /* member springs */
    if (!vr[i].visible)
      continue;
    for (k = 0; k < vr[i].member_count; k++) {
      u32 m = vr[i].members[k];
      double x, y, r0, dx, dy;
      if (m & 0x80000000u)
        continue;
      member_pos(m, &x, &y, &r0);
      dx = vr[i].hx - x;
      dy = vr[i].hy - y;
      ve[m].vx += dx * 0.055;
      ve[m].vy += dy * 0.055;
    }
  }
  for (i = 0; i < ne; i++) {
    if (!ve[i].visible)
      continue;
    ve[i].x += ve[i].vx;
    ve[i].y += ve[i].vy;
  }
}

/* ---- convex hull of the members' padded circles (world -> screen) ---- */

typedef struct {
  double x, y;
} P2;

static int p2_cmp(const void *a, const void *b) {
  const P2 *p = (const P2 *)a, *q = (const P2 *)b;
  if (p->x != q->x)
    return p->x < q->x ? -1 : 1;
  return p->y < q->y ? -1 : p->y > q->y ? 1 : 0;
}

static double cross3(P2 o, P2 a, P2 b) {
  return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
}

/* Andrew's monotone chain; out must hold n+1. Returns hull size. */
static int convex_hull(P2 *pts, int n, P2 *out) {
  int i, k = 0;
  qsort(pts, (size_t)n, sizeof *pts, p2_cmp);
  for (i = 0; i < n; i++) {
    while (k >= 2 && cross3(out[k - 2], out[k - 1], pts[i]) <= 0)
      k--;
    out[k++] = pts[i];
  }
  {
    int lower = k + 1;
    for (i = n - 2; i >= 0; i--) {
      while (k >= lower && cross3(out[k - 2], out[k - 1], pts[i]) <= 0)
        k--;
      out[k++] = pts[i];
    }
  }
  return k - 1;
}

static int sx_of(double wx) {
  return (int)((wx - cam_x) * zoom) + (WIN_W - PANEL_W) / 2;
}
static int sy_of(double wy) { return (int)((wy - cam_y) * zoom) + WIN_H / 2; }

static P2 g_hpts[MAX_HULL], g_hout[MAX_HULL + 1];

static void rel_build_hull(u32 rid) {
  VRel *R = &vr[rid];
  int n = 0, hn, i;
  u32 k;
  double pad = 9.0;
  for (k = 0; k < R->member_count; k++) {
    double x, y, r0;
    member_pos(R->members[k], &x, &y, &r0);
    for (i = 0; i < HULL_SAMPLES; i++) {
      double a = 6.283185307 * i / HULL_SAMPLES;
      if (n < MAX_HULL) {
        g_hpts[n].x = x + cos(a) * (r0 + pad);
        g_hpts[n].y = y + sin(a) * (r0 + pad);
        n++;
      }
    }
  }
  if (n < 3) {
    R->hull_n = 0;
    return;
  }
  hn = convex_hull(g_hpts, n, g_hout);
  R->hull_n = hn;
  R->area = 0;
  for (i = 0; i < hn; i++) {
    P2 a = g_hout[i], b = g_hout[(i + 1) % hn];
    R->area += a.x * b.y - b.x * a.y;
    R->hull[i].x = (short)sx_of(a.x);
    R->hull[i].y = (short)sy_of(a.y);
  }
  R->area = fabs(R->area) * 0.5;
  if (hn < MAX_HULL) {
    R->hull[hn] = R->hull[0]; /* close the loop for XDrawLines */
    R->hull_n = hn + 1;
  }
}

static int point_in_hull(const VRel *R, int px, int py) {
  int i, n = R->hull_n - 1; /* last repeats first */
  int sign = 0;
  if (n < 3)
    return 0;
  for (i = 0; i < n; i++) {
    long ax = R->hull[i].x, ay = R->hull[i].y;
    long bx = R->hull[i + 1].x, by = R->hull[i + 1].y;
    long cr = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
    int s = cr > 0 ? 1 : cr < 0 ? -1 : 0;
    if (s == 0)
      continue;
    if (sign == 0)
      sign = s;
    else if (s != sign)
      return 0;
  }
  return 1;
}

/* ---- kind colors (indexed into an allocated palette) ---- */

static const struct {
  const char *kind;
  unsigned long rgb;
} KIND_COLORS[] = {
    {"decision", 0xE0B341},  {"constraint", 0xD96A6A}, {"question", 0x8F7BE8},
    {"task", 0x5AB0E0},      {"system", 0x53C08A},     {"mechanic", 0x3BA7A0},
    {"pointer", 0x9A8F7A},   {"project", 0xF0F0F0},    {"person", 0xE884C8},
    {"event", 0xC98E4A},     {"commit", 0x7FA35C},     {"file", 0x8A9BA8},
};
enum { N_KINDS = sizeof KIND_COLORS / sizeof KIND_COLORS[0] };

static int kind_index(u32 elem) {
  u32 k = g_graph.elem_kind[elem];
  int i;
  if (k == NONE_U32)
    return -1;
  for (i = 0; i < N_KINDS; i++)
    if ((u32)strlen(KIND_COLORS[i].kind) == elem_name_l(&g_graph, k) &&
        memcmp(KIND_COLORS[i].kind, elem_name(&g_graph, k),
               elem_name_l(&g_graph, k)) == 0)
      return i;
  return -1;
}

/* ---- X state ---- */

static Display *dpy;
static Window win;
static Pixmap back;
static GC gc;
static XFontStruct *fnt;
static unsigned long col_bg, col_panel, col_fg, col_dim, col_grid, col_sel,
    col_live, col_dead, col_nokind, col_handle;
static unsigned long col_kind[N_KINDS];

static unsigned long xcolor(unsigned long rgb) {
  XColor c;
  c.red = (unsigned short)(((rgb >> 16) & 0xFF) * 257);
  c.green = (unsigned short)(((rgb >> 8) & 0xFF) * 257);
  c.blue = (unsigned short)((rgb & 0xFF) * 257);
  c.flags = DoRed | DoGreen | DoBlue;
  XAllocColor(dpy, DefaultColormap(dpy, DefaultScreen(dpy)), &c);
  return c.pixel;
}

static void draw_text(int x, int y, unsigned long col, const char *s, int len) {
  XSetForeground(dpy, gc, col);
  XDrawString(dpy, back, gc, x, y, s, len);
}

/* panel text with word wrap; returns next y. Registers a click row when
 * target_kind is nonzero. */
static int panel_line(int y, unsigned long col, int target_kind, u32 target_id,
                      const char *s) {
  int max_w = PANEL_W - 24, x0 = WIN_W - PANEL_W + 12;
  int len = (int)strlen(s), start = 0, y0 = y;
  if (y > WIN_H)
    return y;
  while (start < len) {
    int fit = len - start, w;
    while (fit > 0 &&
           (w = XTextWidth(fnt, s + start, fit)) > max_w) {
      /* back off to a space when one exists in range */
      int cut = fit - 1;
      while (cut > 0 && s[start + cut] != ' ')
        cut--;
      fit = cut > 0 ? cut : fit - 1;
    }
    if (fit <= 0)
      fit = 1;
    if (y > 24 && y < WIN_H - 4)
      draw_text(x0, y, col, s + start, fit);
    y += 15;
    start += fit;
    while (start < len && s[start] == ' ')
      start++;
  }
  if (target_kind && prow_count < PANEL_ROWS) {
    prows[prow_count].y0 = y0 - 12;
    prows[prow_count].y1 = y - 12 + 12;
    prows[prow_count].kind = target_kind;
    prows[prow_count].id = target_id;
    prow_count++;
  }
  return y;
}

static const char *estr(u32 sid) { return str_ptr(&g_graph.strs, sid); }

static int panel_element(int y, u32 e) {
  char buf[512];
  const Element *el = &g_graph.elements[e];
  u32 link, k;
  snprintf(buf, sizeof buf, "#%u  %s", e, elem_name(&g_graph, e));
  y = panel_line(y, col_fg, 0, 0, buf);
  if (g_graph.elem_kind[e] != NONE_U32) {
    snprintf(buf, sizeof buf, "kind: %s", elem_name(&g_graph, g_graph.elem_kind[e]));
    y = panel_line(y, col_dim, 0, 0, buf);
  }
  for (k = 1; k < el->names.count; k++) {
    snprintf(buf, sizeof buf, "alias: %s", estr(el->names.v[k]));
    y = panel_line(y, col_dim, 0, 0, buf);
  }
  if (el->summary != NONE_U32) {
    y = panel_line(y + 6, col_fg, 0, 0, estr(el->summary));
  }
  snprintf(buf, sizeof buf,
           "conf %.2f  act %.2f  sal %.2f  seen t%u  acc %u",
           el->stats.confidence, el->stats.activation, el->stats.salience,
           el->stats.last_seen, el->stats.access_count);
  y = panel_line(y + 6, col_dim, 0, 0, buf);
  y = panel_line(y + 8, col_sel, 0, 0, "-- relations (click to jump) --");
  {
    int shown = 0, skipped = panel_scroll;
    link = g_graph.rels_by_elem[e];
    while (link != NONE_U32) {
      u32 rid = g_graph.rel_links[link].rel;
      link = g_graph.rel_links[link].next;
      if (!vr[rid].visible)
        continue;
      if (skipped-- > 0)
        continue;
      if (y > WIN_H - 20) {
        y = panel_line(y, col_dim, 0, 0, "... (j/k to scroll)");
        break;
      }
      {
        const Relation *r = &g_graph.relations[rid];
        int n = snprintf(buf, sizeof buf, "rel:%u ", rid);
        for (k = 0; k < r->attr_count && n < (int)sizeof buf - 40; k++) {
          const Term *t = &r->attrs[k].value;
          n += snprintf(buf + n, sizeof buf - (size_t)n, "%s%s=%s",
                        k ? " " : "",
                        elem_name(&g_graph, r->attrs[k].name),
                        t->tag == TERM_ELEM ? elem_name(&g_graph, t->id)
                                            : "rel");
        }
        y = panel_line(y, rel_is_dead(rid) ? col_dead : col_live, 2, rid, buf);
        shown++;
      }
    }
    if (!shown && panel_scroll)
      panel_scroll = 0;
  }
  return y;
}

static int panel_relation(int y, u32 rid) {
  char buf[512];
  const Relation *r = &g_graph.relations[rid];
  u32 k;
  snprintf(buf, sizeof buf, "rel:%u   [%s]", rid, ST_NAMES[r->status]);
  y = panel_line(y, col_fg, 0, 0, buf);
  snprintf(buf, sizeof buf, "created t%u  conf %.2f  support %u (div %u)",
           r->created_at, r->stats.confidence, r->stats.support_count,
           r->stats.support_diversity);
  y = panel_line(y, col_dim, 0, 0, buf);
  y = panel_line(y + 8, col_sel, 0, 0, "-- slots (click to jump) --");
  for (k = 0; k < r->attr_count; k++) {
    const Term *t = &r->attrs[k].value;
    if (t->tag == TERM_ELEM) {
      snprintf(buf, sizeof buf, "%s: %s",
               elem_name(&g_graph, r->attrs[k].name), elem_name(&g_graph, t->id));
      y = panel_line(y, col_fg, 1, t->id, buf);
    } else {
      snprintf(buf, sizeof buf, "%s: rel:%u",
               elem_name(&g_graph, r->attrs[k].name), t->id);
      y = panel_line(y, col_fg, 2, t->id, buf);
    }
  }
  if (r->supporters.count) {
    y = panel_line(y + 8, col_sel, 0, 0, "-- sources --");
    for (k = 0; k < r->supporters.count; k++)
      y = panel_line(y, col_dim, 1, r->supporters.v[k],
                     elem_name(&g_graph, r->supporters.v[k]));
  }
  return y;
}

static void render(void) {
  u32 i;
  char buf[256];
  XSetForeground(dpy, gc, col_bg);
  XFillRectangle(dpy, back, gc, 0, 0, WIN_W, WIN_H);

  /* relation boundaries, biggest first so small ones stay clickable-visible */
  for (i = 0; i < nr; i++)
    if (vr[i].visible)
      rel_build_hull(i);
  for (i = 0; i < nr; i++) {
    int selected = sel_kind == 2 && sel_id == i;
    if (!vr[i].visible || vr[i].hull_n < 4)
      continue;
    XSetForeground(dpy, gc, selected  ? col_sel
                            : rel_is_dead(i) ? col_dead
                                             : col_live);
    XSetLineAttributes(dpy, gc, selected ? 3 : 1,
                       rel_is_dead(i) ? LineOnOffDash : LineSolid, CapRound,
                       JoinRound);
    XDrawLines(dpy, back, gc, vr[i].hull, vr[i].hull_n, CoordModeOrigin);
    /* handle square: the meta-relation attachment point */
    {
      int hx = sx_of(vr[i].hx), hy = sy_of(vr[i].hy);
      XSetForeground(dpy, gc, selected ? col_sel : col_handle);
      XFillRectangle(dpy, back, gc, hx - 2, hy - 2, 5, 5);
    }
  }
  XSetLineAttributes(dpy, gc, 1, LineSolid, CapButt, JoinMiter);

  /* elements */
  for (i = 0; i < ne; i++) {
    int sx, sy, rr, ki;
    if (!ve[i].visible)
      continue;
    sx = sx_of(ve[i].x);
    sy = sy_of(ve[i].y);
    rr = (int)(ve[i].r * zoom);
    if (rr < 3)
      rr = 3;
    if (sx < -rr || sx > WIN_W - PANEL_W + rr || sy < -rr || sy > WIN_H + rr)
      continue;
    ki = kind_index(i);
    XSetForeground(dpy, gc, ki >= 0 ? col_kind[ki] : col_nokind);
    XFillArc(dpy, back, gc, sx - rr, sy - rr, (unsigned)(2 * rr),
             (unsigned)(2 * rr), 0, 360 * 64);
    if (sel_kind == 1 && sel_id == i) {
      XSetForeground(dpy, gc, col_sel);
      XSetLineAttributes(dpy, gc, 3, LineSolid, CapRound, JoinRound);
      XDrawArc(dpy, back, gc, sx - rr - 3, sy - rr - 3, (unsigned)(2 * rr + 6),
               (unsigned)(2 * rr + 6), 0, 360 * 64);
      XSetLineAttributes(dpy, gc, 1, LineSolid, CapButt, JoinMiter);
    }
    if (show_labels && (zoom > 0.55 || (sel_kind == 1 && sel_id == i))) {
      u32 n = elem_name_l(&g_graph, i);
      if (n > 28)
        n = 28;
      draw_text(sx + rr + 4, sy + 4, col_fg, elem_name(&g_graph, i), (int)n);
    }
  }

  /* panel */
  XSetForeground(dpy, gc, col_panel);
  XFillRectangle(dpy, back, gc, WIN_W - PANEL_W, 0, PANEL_W, WIN_H);
  XSetForeground(dpy, gc, col_grid);
  XDrawLine(dpy, back, gc, WIN_W - PANEL_W, 0, WIN_W - PANEL_W, WIN_H);
  prow_count = 0;
  {
    int y = 22;
    if (sel_kind == 1)
      y = panel_element(y, sel_id);
    else if (sel_kind == 2)
      y = panel_relation(y, sel_id);
    else {
      u32 live_r = 0;
      for (i = 0; i < nr; i++)
        if (vr[i].visible)
          live_r++;
      snprintf(buf, sizeof buf, "legend-viz  (build " LEGEND_BUILD ")");
      y = panel_line(y, col_fg, 0, 0, buf);
      snprintf(buf, sizeof buf, "clock %u: %u elements, %u relations shown",
               g_graph.clock, ne, live_r);
      y = panel_line(y, col_dim, 0, 0, buf);
      y = panel_line(y + 10, col_sel, 0, 0, "click an element or a boundary");
      y = panel_line(y + 10, col_dim, 0, 0,
                     "drag/arrows pan | wheel +/- zoom | m meta | d dead | "
                     "v vocab | l labels | r reshake | q quit");
    }
    (void)y;
  }
  XCopyArea(dpy, back, win, gc, 0, 0, WIN_W, WIN_H, 0, 0);
  XFlush(dpy);
}

/* ---- picking ---- */

static void pick(int px, int py) {
  u32 i;
  int p;
  if (px >= WIN_W - PANEL_W) { /* panel rows */
    for (p = 0; p < prow_count; p++)
      if (py >= prows[p].y0 && py <= prows[p].y1) {
        sel_kind = prows[p].kind;
        sel_id = prows[p].id;
        panel_scroll = 0;
        /* center the pick */
        if (sel_kind == 1) {
          cam_x = ve[sel_id].x;
          cam_y = ve[sel_id].y;
        } else {
          cam_x = vr[sel_id].hx;
          cam_y = vr[sel_id].hy;
        }
        return;
      }
    return;
  }
  for (i = 0; i < ne; i++) { /* element circles win over boundaries */
    int sx, sy, rr;
    if (!ve[i].visible)
      continue;
    sx = sx_of(ve[i].x);
    sy = sy_of(ve[i].y);
    rr = (int)(ve[i].r * zoom) + 2;
    if ((px - sx) * (px - sx) + (py - sy) * (py - sy) <= rr * rr) {
      sel_kind = 1;
      sel_id = i;
      panel_scroll = 0;
      return;
    }
  }
  { /* smallest enclosing boundary */
    u32 best = NONE_U32;
    double best_area = 0;
    for (i = 0; i < nr; i++) {
      if (!vr[i].visible || vr[i].hull_n < 4)
        continue;
      if (point_in_hull(&vr[i], px, py) &&
          (best == NONE_U32 || vr[i].area < best_area)) {
        best = i;
        best_area = vr[i].area;
      }
    }
    if (best != NONE_U32) {
      sel_kind = 2;
      sel_id = best;
      panel_scroll = 0;
      return;
    }
  }
  sel_kind = 0;
}

/* Resolve the input — a store dir, a snapshot file, or nothing (CLI-style
 * discovery) — into a store dir snapshot_load can read. A snapshot under any
 * other filename is hard-linked (or copied) as legend.snapshot into a private
 * tmpdir so the validating reader stays the single load path. */
static int resolve_input(const char *arg, char *store, size_t cap) {
  struct stat st;
  if (!arg)
    return discover_store(store, cap);
  if (stat(arg, &st) != 0) {
    fprintf(stderr, "legend-viz: cannot stat %s\n", arg);
    return 0;
  }
  if (S_ISDIR(st.st_mode)) {
    snprintf(store, cap, "%s", arg);
    return 1;
  }
  {
    const char *slash = strrchr(arg, '/');
    const char *base = slash ? slash + 1 : arg;
    if (strcmp(base, "legend.snapshot") == 0) {
      if (slash)
        snprintf(store, cap, "%.*s", (int)(slash - arg), arg);
      else
        snprintf(store, cap, ".");
      return 1;
    }
  }
  {
    static char tmpl[] = "/tmp/legend-viz-XXXXXX";
    char dst[4300];
    if (!mkdtemp(tmpl)) {
      fprintf(stderr, "legend-viz: mkdtemp failed\n");
      return 0;
    }
    snprintf(dst, sizeof dst, "%s/legend.snapshot", tmpl);
    if (link(arg, dst) != 0) { /* cross-device: fall back to a copy */
      FILE *in = fopen(arg, "rb"), *out = in ? fopen(dst, "wb") : NULL;
      char buf[1 << 16];
      size_t n;
      if (!in || !out) {
        fprintf(stderr, "legend-viz: cannot read %s\n", arg);
        return 0;
      }
      while ((n = fread(buf, 1, sizeof buf, in)) > 0)
        fwrite(buf, 1, n, out);
      fclose(in);
      fclose(out);
    }
    snprintf(store, cap, "%s", tmpl);
    return 1;
  }
}

int main(int argc, char **argv) {
  static char store[4200];
  const char *input = NULL;
  int check_only = 0, a;
  u32 i;

  for (a = 1; a < argc; a++) {
    if (strcmp(argv[a], "--check") == 0)
      check_only = 1;
    else if (!input)
      input = argv[a];
    else {
      fprintf(stderr,
              "usage: legend-viz [store-dir | legend.snapshot] [--check]\n");
      return 1;
    }
  }
  if (!resolve_input(input, store, sizeof store)) {
    if (!input)
      fprintf(stderr, "legend-viz: no store found (pass a path or set "
                      "LEGEND_STATE_DIR)\n");
    return 1;
  }
  g_err_trap = 1;
  if (setjmp(g_err_jmp)) {
    fprintf(stderr, "legend-viz: %s\n", g_err.message);
    return 1;
  }
  if (!snapshot_load(&g_graph, store)) {
    fprintf(stderr, "legend-viz: no snapshot in %s\n", store);
    return 1;
  }
  g_err_trap = 0;

  ne = g_graph.element_count;
  nr = g_graph.relation_count;
  ve = calloc(ne ? ne : 1, sizeof *ve);
  vr = calloc(nr ? nr : 1, sizeof *vr);
  if (!ve || !vr) {
    fprintf(stderr, "legend-viz: out of memory\n");
    return 1;
  }
  for (i = 0; i < nr; i++)
    collect_members(i);
  recompute_visibility();
  layout_seed();
  for (i = 0; i < 400; i++)
    layout_step();

  if (check_only) {
    u32 vis_e = 0, vis_r = 0;
    for (i = 0; i < ne; i++)
      vis_e += (u32)ve[i].visible;
    for (i = 0; i < nr; i++)
      vis_r += (u32)vr[i].visible;
    printf("legend-viz --check: store %s\n", store);
    printf("elements %u (%u visible), relations %u (%u visible), clock %u\n",
           ne, vis_e, nr, vis_r, g_graph.clock);
    return 0;
  }

  dpy = XOpenDisplay(NULL);
  if (!dpy) {
    fprintf(stderr, "legend-viz: cannot open display\n");
    return 1;
  }
  {
    int scr = DefaultScreen(dpy);
    win = XCreateSimpleWindow(dpy, RootWindow(dpy, scr), 0, 0, WIN_W, WIN_H, 0,
                              0, 0);
    back = XCreatePixmap(dpy, win, WIN_W, WIN_H,
                         (unsigned)DefaultDepth(dpy, scr));
    gc = XCreateGC(dpy, back, 0, NULL);
    fnt = XLoadQueryFont(dpy, "-*-fixed-medium-r-*-*-13-*-*-*-*-*-*-*");
    if (!fnt)
      fnt = XLoadQueryFont(dpy, "fixed");
    if (!fnt) {
      fprintf(stderr, "legend-viz: no usable X font\n");
      return 1;
    }
    XSetFont(dpy, gc, fnt->fid);
    XStoreName(dpy, win, "legend-viz");
    XSelectInput(dpy, win,
                 ExposureMask | KeyPressMask | ButtonPressMask |
                     ButtonReleaseMask | PointerMotionMask |
                     StructureNotifyMask);
    XMapWindow(dpy, win);
  }
  col_bg = xcolor(0x14161A);
  col_panel = xcolor(0x1D2026);
  col_fg = xcolor(0xE8E6E3);
  col_dim = xcolor(0x8A8F98);
  col_grid = xcolor(0x30343B);
  col_sel = xcolor(0xFFD75F);
  col_live = xcolor(0x4A5568);
  col_dead = xcolor(0x7A3B3B);
  col_nokind = xcolor(0x6B7280);
  col_handle = xcolor(0x565E6B);
  for (i = 0; i < N_KINDS; i++)
    col_kind[i] = xcolor(KIND_COLORS[i].rgb);

  {
    int dragging = 0, drag_moved = 0, lx = 0, ly = 0, running = 1;
    int settle = 120;
    while (running) {
      while (XPending(dpy)) {
        XEvent ev;
        XNextEvent(dpy, &ev);
        switch (ev.type) {
        case ButtonPress:
          if (ev.xbutton.button == Button1) {
            dragging = 1;
            drag_moved = 0;
            lx = ev.xbutton.x;
            ly = ev.xbutton.y;
          } else if (ev.xbutton.button == Button4) {
            zoom *= 1.12;
          } else if (ev.xbutton.button == Button5) {
            zoom /= 1.12;
          }
          break;
        case ButtonRelease:
          if (ev.xbutton.button == Button1) {
            dragging = 0;
            if (!drag_moved)
              pick(ev.xbutton.x, ev.xbutton.y);
          }
          break;
        case MotionNotify:
          if (dragging) {
            int dx = ev.xmotion.x - lx, dy = ev.xmotion.y - ly;
            if (abs(dx) + abs(dy) > 2)
              drag_moved = 1;
            cam_x -= dx / zoom;
            cam_y -= dy / zoom;
            lx = ev.xmotion.x;
            ly = ev.xmotion.y;
          }
          break;
        case KeyPress: {
          KeySym k = XLookupKeysym(&ev.xkey, 0);
          double pan = 60.0 / zoom;
          if (k == XK_q || k == XK_Escape) {
            if (k == XK_Escape && sel_kind)
              sel_kind = 0;
            else
              running = 0;
          } else if (k == XK_Left)
            cam_x -= pan;
          else if (k == XK_Right)
            cam_x += pan;
          else if (k == XK_Up)
            cam_y -= pan;
          else if (k == XK_Down)
            cam_y += pan;
          else if (k == XK_plus || k == XK_equal)
            zoom *= 1.2;
          else if (k == XK_minus)
            zoom /= 1.2;
          else if (k == XK_j)
            panel_scroll++;
          else if (k == XK_k && panel_scroll > 0)
            panel_scroll--;
          else if (k == XK_m || k == XK_d || k == XK_v) {
            if (k == XK_m)
              show_meta = !show_meta;
            if (k == XK_d)
              show_dead = !show_dead;
            if (k == XK_v)
              show_vocab = !show_vocab;
            recompute_visibility();
            settle = 200;
          } else if (k == XK_l)
            show_labels = !show_labels;
          else if (k == XK_r) {
            layout_seed();
            settle = 400;
          }
          break;
        }
        }
      }
      if (settle > 0) {
        layout_step();
        settle--;
      }
      render();
      {
        struct timespec nap = {0, 16 * 1000 * 1000};
        nanosleep(&nap, NULL);
      }
    }
  }
  XCloseDisplay(dpy);
  return 0;
}
