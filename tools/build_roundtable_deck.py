"""Build the Roundtable AI executive concept deck (clickable PDF).

Run:  python tools/build_roundtable_deck.py
Out:  output/pdf/roundtable_ai_executive_overview.pdf
"""
from __future__ import annotations

import os
from math import cos, pi, sin, atan2, degrees

from reportlab.lib.colors import Color, HexColor
from reportlab.lib.units import inch
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "output", "pdf", "roundtable_ai_executive_overview.pdf")
W, H = 13.333 * inch, 7.5 * inch
M = 48  # page margin

# ---------------------------------------------------------------- palette
NAVY = HexColor("#071522")
PANEL = HexColor("#102B40")
PANEL_2 = HexColor("#15364E")
TEAL = HexColor("#2DD4BF")
CYAN = HexColor("#67E8F9")
GOLD = HexColor("#F9C74F")
CORAL = HexColor("#FB7185")
GREEN = HexColor("#7DD3A7")
VIOLET = HexColor("#C4B5FD")
MUTED = HexColor("#9CB2C3")
TEXT = HexColor("#F4F8FB")
INK = HexColor("#D9E6EE")
GRID = HexColor("#24465D")
DARK_TEXT = HexColor("#0B1B29")

AGENTS = [  # name, colour
    ("AI-A", TEAL),
    ("AI-B", CYAN),
    ("AI-C", GOLD),
    ("AI-D", CORAL),
]

PAGES = [  # key, outline title, short nav label
    ("cover", "Cover", "Cover"),
    ("overview", "60-second overview", "Overview"),
    ("table", "The round table", "Round table"),
    ("matrix", "Review matrix", "Reviews"),
    ("memory", "Memory isolation", "Memory"),
    ("improve", "Improve & synthesise", "Improve"),
    ("numbers", "By the numbers", "Numbers"),
    ("why", "Why it matters & next steps", "Next steps"),
]


# ---------------------------------------------------------------- fonts
def register_fonts():
    cands = {
        "UI": r"C:\Windows\Fonts\segoeui.ttf",
        "UI-SB": r"C:\Windows\Fonts\seguisb.ttf",
        "UI-B": r"C:\Windows\Fonts\segoeuib.ttf",
        "UI-L": r"C:\Windows\Fonts\segoeuil.ttf",
    }
    for name, path in cands.items():
        if os.path.exists(path):
            pdfmetrics.registerFont(TTFont(name, path))
    reg = pdfmetrics.getRegisteredFontNames()
    f = "UI" if "UI" in reg else "Helvetica"
    fb = "UI-B" if "UI-B" in reg else "Helvetica-Bold"
    fsb = "UI-SB" if "UI-SB" in reg else fb
    fl = "UI-L" if "UI-L" in reg else f
    return f, fsb, fb, fl


FONT, FONT_SB, FONT_B, FONT_L = register_fonts()


# ---------------------------------------------------------------- helpers
def alpha(color, a):
    return Color(color.red, color.green, color.blue, alpha=a)


def sw(text, font, size):
    return pdfmetrics.stringWidth(text, font, size)


def wrap(text, font, size, width):
    lines, cur = [], ""
    for word in text.split():
        trial = word if not cur else cur + " " + word
        if sw(trial, font, size) <= width:
            cur = trial
        else:
            if cur:
                lines.append(cur)
            cur = word
    if cur:
        lines.append(cur)
    return lines


def text(c, s, x, y, size=12, font=None, color=INK, align="left"):
    font = font or FONT
    c.setFont(font, size)
    c.setFillColor(color)
    if align == "left":
        c.drawString(x, y, s)
    elif align == "center":
        c.drawCentredString(x, y, s)
    else:
        c.drawRightString(x, y, s)


def para(c, s, x, y, width, size=12, leading=None, font=None, color=INK,
         align="left"):
    """Draw wrapped paragraph; returns y below last line."""
    font = font or FONT
    leading = leading or size * 1.38
    for line in wrap(s, font, size, width):
        text(c, line, x, y, size, font, color, align)
        y -= leading
    return y


def box(c, x, y, w, h, fill=PANEL, stroke=None, r=12, lw=1):
    c.setLineWidth(lw)
    c.setFillColor(fill)
    if stroke is None:
        c.roundRect(x, y, w, h, r, stroke=0, fill=1)
    else:
        c.setStrokeColor(stroke)
        c.roundRect(x, y, w, h, r, stroke=1, fill=1)


def chip(c, s, x, y, color=TEAL, size=9, pad=8):
    w = sw(s, FONT_SB, size) + pad * 2
    box(c, x, y, w, size + 10, fill=alpha(color, 0.14), stroke=alpha(color, 0.5), r=(size + 10) / 2, lw=0.8)
    text(c, s, x + pad, y + 5, size, FONT_SB, color)
    return w


def arrow(c, x1, y1, x2, y2, color=MUTED, lw=1.4, head=7, dash=None):
    c.setStrokeColor(color)
    c.setFillColor(color)
    c.setLineWidth(lw)
    if dash:
        c.setDash(dash)
    ang = atan2(y2 - y1, x2 - x1)
    # shorten line so it doesn't poke through the head
    ex, ey = x2 - cos(ang) * head * 0.8, y2 - sin(ang) * head * 0.8
    c.line(x1, y1, ex, ey)
    c.setDash()
    arrowhead(c, x2, y2, ang, color, head)


def arrowhead(c, x, y, ang, color, head=7):
    c.setFillColor(color)
    c.setStrokeColor(color)
    p = c.beginPath()
    p.moveTo(x, y)
    p.lineTo(x - head * cos(ang - 0.45), y - head * sin(ang - 0.45))
    p.lineTo(x - head * cos(ang + 0.45), y - head * sin(ang + 0.45))
    p.close()
    c.drawPath(p, stroke=0, fill=1)


def arc_arrow(c, cx, cy, r, a0, a1, color, lw=1.6, head=8):
    """Arrow along a circle from angle a0 to a1 (degrees, ccw positive)."""
    c.setStrokeColor(color)
    c.setLineWidth(lw)
    extent = a1 - a0
    c.arc(cx - r, cy - r, cx + r, cy + r, a0, extent)
    t = a1 * pi / 180
    ex, ey = cx + r * cos(t), cy + r * sin(t)
    tang = t + (pi / 2 if extent > 0 else -pi / 2)
    arrowhead(c, ex, ey, tang, color, head)


def node(c, x, y, r, label, color, fill=PANEL_2, size=12, lw=2.2, sub=None):
    c.setLineWidth(lw)
    c.setStrokeColor(color)
    c.setFillColor(fill)
    c.circle(x, y, r, stroke=1, fill=1)
    text(c, label, x, y - size * 0.36, size, FONT_B, color, "center")
    if sub:
        text(c, sub, x, y - r - 13, 8.5, FONT, MUTED, "center")


def paper(c, x, y, w, h, color, title, lines=3, fill=None):
    """A little document icon."""
    box(c, x, y, w, h, fill=fill or alpha(color, 0.12), stroke=alpha(color, 0.75), r=4, lw=1)
    text(c, title, x + w / 2, y + h - 12, 7.5, FONT_B, color, "center")
    c.setStrokeColor(alpha(color, 0.55))
    c.setLineWidth(0.8)
    for i in range(lines):
        yy = y + h - 20 - i * 6
        c.line(x + 6, yy, x + w - 6 - (4 if i == lines - 1 else 0), yy)


def link(c, key, rect):
    c.linkRect("", key, rect, relative=0, thickness=0)


def envelope(c, x, y, w, h, color):
    box(c, x, y, w, h, fill=alpha(color, 0.15), stroke=color, r=3, lw=1)
    c.setStrokeColor(color)
    c.setLineWidth(1)
    c.line(x, y + h, x + w / 2, y + h * 0.42)
    c.line(x + w, y + h, x + w / 2, y + h * 0.42)


# ---------------------------------------------------------------- chrome
def chrome(c, idx, title_chip, headline, sub=None):
    key = PAGES[idx][0]
    c.bookmarkPage(key)
    c.addOutlineEntry(PAGES[idx][1], key, level=0)
    # background
    c.setFillColor(NAVY)
    c.rect(0, 0, W, H, stroke=0, fill=1)
    c.setFillColor(alpha(TEAL, 0.05))
    c.circle(W - 60, H + 30, 250, stroke=0, fill=1)
    c.setFillColor(alpha(CYAN, 0.04))
    c.circle(-40, -60, 220, stroke=0, fill=1)
    # header
    chip(c, title_chip.upper(), M, H - 52, TEAL, 9)
    text(c, headline, M, H - 92, 27, FONT_B, TEXT)
    if sub:
        text(c, sub, M, H - 114, 13, FONT, MUTED)
    footer(c, idx)


def footer(c, idx):
    c.setStrokeColor(GRID)
    c.setLineWidth(0.6)
    c.line(M, 40, W - M, 40)
    text(c, "ROUNDTABLE AI  |  EXECUTIVE CONCEPT", M, 22, 8, FONT_SB, MUTED)
    text(c, f"{idx + 1:02d} / {len(PAGES):02d}", W - M, 22, 8, FONT_SB, MUTED, "right")
    # nav dots (clickable)
    n = len(PAGES)
    gap = 16
    x0 = W / 2 - (n - 1) * gap / 2
    for i, (k, _t, label) in enumerate(PAGES):
        x = x0 + i * gap
        c.setFillColor(TEAL if i == idx else GRID)
        c.circle(x, 26, 3.2 if i == idx else 2.6, stroke=0, fill=1)
        link(c, k, (x - 8, 16, x + 8, 36))
    # prev / next
    if idx > 0:
        text(c, "<  PREV", W / 2 - 120, 22, 8, FONT_SB, MUTED, "right")
        link(c, PAGES[idx - 1][0], (W / 2 - 170, 14, W / 2 - 115, 36))
    if idx < n - 1:
        text(c, "NEXT  >", W / 2 + 120, 22, 8, FONT_SB, MUTED, "left")
        link(c, PAGES[idx + 1][0], (W / 2 + 115, 14, W / 2 + 170, 36))
    text(c, "menu", W - M - 70, 22, 8, FONT_SB, alpha(MUTED, 0.8), "right")
    link(c, "overview", (W - M - 100, 14, W - M - 66, 36))


# ---------------------------------------------------------------- pages
def page_cover(c):
    c.bookmarkPage("cover")
    c.addOutlineEntry("Cover", "cover", level=0)
    c.setFillColor(NAVY)
    c.rect(0, 0, W, H, stroke=0, fill=1)
    c.setFillColor(alpha(TEAL, 0.06))
    c.circle(W - 120, H + 20, 300, stroke=0, fill=1)
    c.setFillColor(alpha(CORAL, 0.05))
    c.circle(-60, -80, 260, stroke=0, fill=1)

    chip(c, "EXECUTIVE CONCEPT", M, H - 60, TEAL, 9)
    text(c, "Roundtable AI", M, H - 135, 52, FONT_B, TEXT)
    text(c, "Independent minds. Isolated reviews. One stronger answer.", M, H - 168, 19, FONT_SB, CYAN)
    para(c, "A controlled multi-model decision system. Several AIs answer the same "
            "question alone, review each other anonymously in fresh sessions, improve "
            "only their own work, and a neutral fifth AI combines the improved results.",
         M, H - 205, 430, 13, 19, FONT, INK)

    # CTA button
    bx, by, bw, bh = M, 110, 250, 46
    box(c, bx, by, bw, bh, fill=TEAL, r=10)
    text(c, "OPEN THE 60-SECOND OVERVIEW", bx + bw / 2, by + 17, 11, FONT_B, DARK_TEXT, "center")
    link(c, "overview", (bx, by, bx + bw, by + bh))
    text(c, "Clickable concept deck  |  September 2026  |  use the dots or arrows at the bottom to navigate",
         M, 80, 9, FONT, MUTED)

    # round table graphic
    cx, cy, R = W - 245, H / 2 + 12, 124
    c.setFillColor(alpha(PANEL_2, 0.7))
    c.circle(cx, cy, R + 52, stroke=0, fill=1)
    c.setStrokeColor(GRID)
    c.setLineWidth(1.2)
    c.circle(cx, cy, R, stroke=1, fill=0)
    # centre question card
    qw, qh = 122, 58
    box(c, cx - qw / 2, cy - qh / 2, qw, qh, fill=TEAL, r=12)
    text(c, "ONE QUESTION", cx, cy + 12, 8, FONT_SB, DARK_TEXT, "center")
    text(c, "4 independent views", cx, cy - 8, 11.5, FONT_B, DARK_TEXT, "center")
    angles = [90, 0, -90, 180]
    for (name, col), a in zip(AGENTS, angles):
        t = a * pi / 180
        nx, ny = cx + R * cos(t), cy + R * sin(t)
        node(c, nx, ny, 26, name, col, size=11)
        # arrow from node toward card
        sx, sy = cx + (R - 30) * cos(t), cy + (R - 30) * sin(t)
        ex, ey = cx + (qw / 2 + 14) * cos(t) if a in (0, 180) else cx, \
                 cy + (qh / 2 + 14) * sin(t) if a in (90, -90) else cy
        arrow(c, sx, sy, ex, ey, col, 1.3, 6)
    # rotation arcs
    for a in (45, -45, -135, 135):
        arc_arrow(c, cx, cy, R + 36, a + 22, a - 22, alpha(MUTED, 0.6), 1.1, 6)
    text(c, "answers rotate, memory does not", cx, cy - R - 66, 9, FONT, MUTED, "center")

    footer(c, 0)


def page_overview(c):
    chrome(c, 1, "60-second overview", "Five steps, one stronger answer",
           "Click any step to jump to its detail page.")
    steps = [
        ("1", "ASK", "One question is sent to four different AIs at the same time.", TEAL, "table"),
        ("2", "ANSWER ALONE", "Each AI writes its answer privately. No AI sees another's work.", CYAN, "table"),
        ("3", "SWAP CHAIRS", "Every answer is reviewed by the other three AIs in fresh, anonymous sessions.", GOLD, "matrix"),
        ("4", "GO HOME & IMPROVE", "Each AI gets its own answer plus three reviews, self-evaluates and rewrites.", CORAL, "improve"),
        ("5", "FINAL SYNTHESIS", "A neutral fifth AI merges the four improved answers into one final answer.", VIOLET, "improve"),
    ]
    n = len(steps)
    gap = 14
    cw = (W - 2 * M - gap * (n - 1)) / n
    ch = 220
    y = H - 150 - ch
    for i, (num, title, body, col, dest) in enumerate(steps):
        x = M + i * (cw + gap)
        box(c, x, y, cw, ch, fill=PANEL, stroke=alpha(col, 0.35), r=14)
        c.setFillColor(col)
        c.circle(x + 26, y + ch - 30, 15, stroke=0, fill=1)
        text(c, num, x + 26, y + ch - 35, 13, FONT_B, DARK_TEXT, "center")
        text(c, title, x + 50, y + ch - 35, 11, FONT_B, col)
        para(c, body, x + 16, y + ch - 70, cw - 32, 11.5, 16.5, FONT, INK)
        text(c, "view detail  >", x + 16, y + 16, 8.5, FONT_SB, col)
        link(c, dest, (x, y, x + cw, y + ch))
        if i < n - 1:
            arrow(c, x + cw + 1, y + ch / 2, x + cw + gap - 1, y + ch / 2, MUTED, 1.2, 6)

    # golden rule band
    by = y - 78
    box(c, M, by, W - 2 * M, 58, fill=alpha(TEAL, 0.10), stroke=alpha(TEAL, 0.45), r=12)
    text(c, "THE ONE RULE", M + 22, by + 33, 9, FONT_B, TEAL)
    para(c, "Memory never carries over between steps. Every review and every rewrite is a brand-new, "
            "isolated AI session that receives only the papers it is allowed to read. Independence is "
            "enforced by the software, not by asking the AI nicely.",
         M + 22, by + 19 + 3, W - 2 * M - 44, 10.5, 14, FONT, INK)


def page_table(c):
    chrome(c, 2, "Steps 1-3", "The round table: write privately, then swap chairs",
           "Four AIs, four chairs, four papers. The papers stay on the chairs; the AIs move.")
    # ---- table diagram (left)
    cx, cy, R = 290, H / 2 - 26, 96
    c.setFillColor(alpha(PANEL_2, 0.8))
    c.circle(cx, cy, R + 64, stroke=0, fill=1)
    c.setFillColor(PANEL)
    c.setStrokeColor(GRID)
    c.setLineWidth(1.2)
    c.circle(cx, cy, 44, stroke=1, fill=1)
    text(c, "ROUND", cx, cy + 4, 9, FONT_B, MUTED, "center")
    text(c, "TABLE", cx, cy - 8, 9, FONT_B, MUTED, "center")
    angles = [90, 0, -90, 180]
    for (name, col), a in zip(AGENTS, angles):
        t = a * pi / 180
        px, py = cx + (R - 24) * cos(t), cy + (R - 24) * sin(t)
        paper(c, px - 22, py - 22, 44, 44, col, "Answer " + name[-1])
        nx, ny = cx + (R + 40) * cos(t), cy + (R + 40) * sin(t)
        node(c, nx, ny, 24, name, col, size=10)
    # rotation arcs (clockwise = decreasing angle)
    for a in (45, -45, -135, 135):
        arc_arrow(c, cx, cy, R + 40, a + 26, a - 26, TEAL, 1.5, 7)
    text(c, "AIs rotate clockwise; each chair's paper stays put", cx, cy - R - 82, 9.5, FONT_SB, TEAL, "center")

    # ---- right side steps
    x = 560
    y = H - 150
    items = [
        ("ROUND 0  |  WRITE", TEAL,
         "Every AI answers the question alone and leaves its answer on its own chair. "
         "Nobody has seen anybody else's answer."),
        ("ROTATION 1, 2, 3  |  REVIEW", GOLD,
         "Each AI moves one chair, reads only the paper it finds there and writes a review: "
         "what is right, what is wrong, what is missing. Then it moves again."),
        ("FRESH MEMORY AT EVERY CHAIR", CORAL,
         "Technically each review is a brand-new AI call. The reviewer does not remember its own "
         "answer, the previous chair, or who wrote the paper. Names are hidden."),
        ("BACK HOME", CYAN,
         "After three rotations every AI is back at its own chair, and every answer now carries "
         "three independent reviews."),
    ]
    for title, col, body in items:
        c.setFillColor(col)
        c.circle(x + 5, y - 4, 4, stroke=0, fill=1)
        text(c, title, x + 18, y - 8, 10.5, FONT_B, col)
        y = para(c, body, x + 18, y - 26, W - M - x - 18, 11, 15.5, FONT, INK)
        y -= 14
    text(c, "See the full review matrix  >", x + 18, 62, 9.5, FONT_SB, TEAL)
    link(c, "matrix", (x + 18, 54, x + 200, 74))


def page_matrix(c):
    chrome(c, 3, "Step 3 in detail", "Review matrix: every answer, reviewed by every other AI",
           "Twelve reviews, twelve separate sessions. Reviewers never see their own answer.")
    # grid
    gx, gy = M + 110, H - 175
    cell = 74
    text(c, "ANSWER BEING REVIEWED", gx + 2 * cell, gy + 34, 8.5, FONT_B, MUTED, "center")
    c.saveState()
    c.translate(gx - 92, gy - 2 * cell)
    c.rotate(90)
    text(c, "REVIEWER", 0, 0, 8.5, FONT_B, MUTED, "center")
    c.restoreState()
    for j, (name, col) in enumerate(AGENTS):
        text(c, name, gx + j * cell + cell / 2, gy + 12, 10.5, FONT_B, col, "center")
        text(c, name, gx - 14, gy - (j + 1) * cell + cell / 2 - 4, 10.5, FONT_B, col, "right")
    for i, (rname, rcol) in enumerate(AGENTS):
        for j, (aname, acol) in enumerate(AGENTS):
            x = gx + j * cell + 3
            y = gy - (i + 1) * cell + 3
            if i == j:
                box(c, x, y, cell - 6, cell - 6, fill=alpha(GRID, 0.35), stroke=alpha(GRID, 0.8), r=8)
                text(c, "own answer", x + (cell - 6) / 2, y + (cell - 6) / 2 + 2, 7.5, FONT_SB, MUTED, "center")
                text(c, "skipped", x + (cell - 6) / 2, y + (cell - 6) / 2 - 9, 7.5, FONT, MUTED, "center")
            else:
                box(c, x, y, cell - 6, cell - 6, fill=alpha(acol, 0.10), stroke=alpha(acol, 0.55), r=8)
                text(c, f"{rname[-1]} reviews {aname[-1]}", x + (cell - 6) / 2, y + (cell - 6) / 2 + 6, 8.5, FONT_B, acol, "center")
                text(c, "fresh session", x + (cell - 6) / 2, y + (cell - 6) / 2 - 7, 7, FONT, MUTED, "center")
    text(c, "12 reviews  =  4 answers x 3 reviewers each", gx, gy - 4 * cell - 20, 9.5, FONT_SB, TEAL)

    # right: envelope contents
    x = 480
    y = H - 150
    bw = W - M - x
    box(c, x, y - 140, bw, 140, fill=PANEL, stroke=alpha(GREEN, 0.5), r=14)
    text(c, "WHAT A REVIEWER RECEIVES", x + 20, y - 26, 10, FONT_B, GREEN)
    for k, s in enumerate(["The original question",
                           "Exactly one answer, with the author's name removed",
                           "Its own reviewer system prompt and a fixed review template"]):
        text(c, "+", x + 22, y - 52 - k * 20, 12, FONT_B, GREEN)
        text(c, s, x + 38, y - 50 - k * 20, 11, FONT, INK)
    text(c, "Output: correct points, problems found, missing items, proposed fix, confidence.",
         x + 20, y - 124, 9.5, FONT, MUTED)

    y2 = y - 156
    box(c, x, y2 - 140, bw, 140, fill=PANEL, stroke=alpha(CORAL, 0.5), r=14)
    text(c, "WHAT IT NEVER SEES", x + 20, y2 - 26, 10, FONT_B, CORAL)
    for k, s in enumerate(["Its own original answer",
                           "The other two answers, or any earlier review",
                           "Which model or company wrote the answer",
                           "Any chat history from previous chairs"]):
        text(c, "x", x + 22, y2 - 50 - k * 20, 11, FONT_B, CORAL)
        text(c, s, x + 38, y2 - 50 - k * 20, 11, FONT, INK)
    text(c, "All 12 reviews can run at the same time, so this step feels like one wait, not twelve.",
         x, 62, 9.5, FONT_SB, TEAL)


def page_memory(c):
    chrome(c, 4, "The core rule", "Memory isolation: the controller owns the papers, not the AIs",
           "Before every call, the controller builds a sealed envelope containing only what that AI may read.")
    # ---- envelope diagram (left)
    lx, ly = M, H - 150
    lw_ = 380
    box(c, lx, ly - 300, lw_, 300, fill=PANEL, r=14)
    # ledger
    bx, by, bw, bh = lx + 20, ly - 110, 150, 84
    box(c, bx, by, bw, bh, fill=PANEL_2, stroke=TEAL, r=10)
    text(c, "ARTIFACT LEDGER", bx + bw / 2, by + bh - 20, 9, FONT_B, TEAL, "center")
    for k, s in enumerate(["question", "4 answers", "12 reviews", "4 improved"]):
        text(c, s, bx + bw / 2, by + bh - 36 - k * 13, 8.5, FONT, INK, "center")
    text(c, "append-only, nothing is ever edited", bx + bw / 2, by - 12, 7.5, FONT, MUTED, "center")
    # controller
    kx, ky, kw, kh = lx + 20, ly - 250, 150, 84
    box(c, kx, ky, kw, kh, fill=PANEL_2, stroke=CYAN, r=10)
    text(c, "CONTROLLER", kx + kw / 2, ky + kh - 20, 9, FONT_B, CYAN, "center")
    for k, s in enumerate(["reads the ledger", "picks allowed papers", "seals an envelope", "starts a fresh call"]):
        text(c, s, kx + kw / 2, ky + kh - 36 - k * 13, 8.5, FONT, INK, "center")
    arrow(c, bx + bw / 2, by, kx + kw / 2, ky + kh, MUTED, 1.2, 6)
    # envelopes to agents
    ex0 = lx + 215
    for k, (name, col) in enumerate(AGENTS):
        ey = ly - 45 - k * 66
        envelope(c, ex0, ey - 14, 44, 28, col)
        arrow(c, kx + kw + 4, ky + kh / 2 + (ey - (ky + kh / 2)) * 0.15, ex0 - 4, ey, alpha(col, 0.8), 1.0, 5)
        node(c, ex0 + 100, ey, 20, name, col, size=9)
        arrow(c, ex0 + 48, ey, ex0 + 76, ey, alpha(col, 0.8), 1.0, 5)
        text(c, "fresh call", ex0 + 100, ey - 32, 7, FONT, MUTED, "center")
    text(c, "one envelope per call, then the session is destroyed", lx + lw_ / 2, ly - 288, 8.5, FONT_SB, MUTED, "center")

    # ---- table (right)
    tx = lx + lw_ + 28
    tw = W - M - tx
    cols = [0.24, 0.40, 0.36]
    cws = [tw * f for f in cols]
    ty = H - 150
    rh = 60
    heads = ["STAGE", "MAY SEE", "MUST NOT SEE"]
    box(c, tx, ty - 26, tw, 26, fill=PANEL_2, r=6)
    xx = tx
    for h_, cw_ in zip(heads, cws):
        text(c, h_, xx + 12, ty - 18, 8.5, FONT_B, TEAL)
        xx += cw_
    rows = [
        ("Initial answer", "Question + its own system prompt", "Any other AI's answer"),
        ("Peer review", "Question + one anonymous answer", "Its own answer, earlier reviews, other answers, author identity"),
        ("Improve own answer", "Question + its own original answer + the 3 reviews written about it", "Other AIs' answers and the reviews it wrote for others"),
        ("Final synthesis", "Question + 4 improved answers in random order", "Raw reviews, chat history, model names"),
    ]
    y = ty - 26
    for r_i, row in enumerate(rows):
        y -= rh
        if r_i % 2 == 0:
            box(c, tx, y, tw, rh, fill=alpha(PANEL, 0.7), r=0)
        xx = tx
        for j, (cell_text, cw_) in enumerate(zip(row, cws)):
            col = TEXT if j == 0 else (GREEN if j == 1 else CORAL)
            font = FONT_SB if j == 0 else FONT
            lines = wrap(cell_text, font, 9.5, cw_ - 22)
            yy = y + rh / 2 + (len(lines) - 1) * 6.5 - 3
            for ln in lines:
                text(c, ln, xx + 12, yy, 9.5, font, col)
                yy -= 13
            xx += cw_
    c.setStrokeColor(GRID)
    c.setLineWidth(0.6)
    c.line(tx, y, tx + tw, y)
    para(c, "Enforced in code: no shared chat thread, no conversation IDs passed between stages, canary "
            "markers in test runs to prove nothing leaks.",
         tx, y - 22, tw, 9.5, 13, FONT_SB, TEAL)


def page_improve(c):
    chrome(c, 5, "Steps 4-5", "Go home, improve, then one neutral AI writes the final answer",
           "Each AI upgrades only its own work. A fifth AI, outside the panel, combines the results.")
    # ---- left: improve lanes
    lx = M
    y0 = H - 150
    lane_h = 58
    text(c, "STEP 4  |  SELF-EVALUATE AND IMPROVE", lx, y0 + 6, 9.5, FONT_B, CORAL)
    improved_pts = []
    for k, (name, col) in enumerate(AGENTS):
        y = y0 - 24 - k * lane_h
        # own answer paper
        paper(c, lx, y - 40, 34, 44, col, "Ans " + name[-1])
        text(c, "+", lx + 44, y - 24, 14, FONT_B, MUTED)
        # three reviews
        others = [a for a in AGENTS if a[0] != name]
        for m, (oname, ocol) in enumerate(others):
            paper(c, lx + 60 + m * 30, y - 40, 26, 44, ocol, "rev " + oname[-1], lines=2)
        # arrow to node
        arrow(c, lx + 152, y - 18, lx + 182, y - 18, MUTED, 1.2, 6)
        node(c, lx + 204, y - 18, 20, name, col, size=9)
        arrow(c, lx + 228, y - 18, lx + 256, y - 18, MUTED, 1.2, 6)
        paper(c, lx + 260, y - 40, 50, 44, col, "Improved " + name[-1], fill=alpha(col, 0.22))
        improved_pts.append((lx + 310, y - 18, col))
    text(c, "own answer + 3 reviews about it", lx, y0 - 24 - 4 * lane_h + 4, 8, FONT, MUTED)
    text(c, "fresh call", lx + 204, y0 - 24 - 4 * lane_h + 4, 8, FONT, MUTED, "center")
    text(c, "improved", lx + 285, y0 - 24 - 4 * lane_h + 4, 8, FONT, MUTED, "center")

    # ---- right: synthesis
    sx = 560
    text(c, "STEP 5  |  FINAL SYNTHESIS", sx, y0 + 6, 9.5, FONT_B, VIOLET)
    fx, fy = sx + 150, y0 - 24 - 1.5 * lane_h - 18
    for (px, py, col) in improved_pts:
        arrow(c, px + 6, py, fx - 42, fy + (py - fy) * 0.25, alpha(col, 0.8), 1.2, 6)
    node(c, fx, fy, 40, "FINAL", VIOLET, fill=alpha(VIOLET, 0.15), size=11, lw=2.5)
    text(c, "AI", fx, fy - 16, 9, FONT_B, VIOLET, "center")
    text(c, "5th model, not on the panel", fx, fy - 54, 8, FONT, MUTED, "center")
    arrow(c, fx + 44, fy, fx + 84, fy, VIOLET, 1.6, 8)
    box(c, fx + 88, fy - 34, 118, 68, fill=alpha(VIOLET, 0.18), stroke=VIOLET, r=10)
    text(c, "FINAL ANSWER", fx + 147, fy + 12, 9.5, FONT_B, VIOLET, "center")
    text(c, "with provenance", fx + 147, fy - 4, 8, FONT, INK, "center")
    text(c, "and open questions", fx + 147, fy - 16, 8, FONT, INK, "center")

    # what the synthesizer does
    y = 190
    box(c, sx, 62, W - M - sx, y - 62, fill=PANEL, r=12)
    text(c, "THE FINAL AI MUST", sx + 18, y - 22, 9, FONT_B, VIOLET)
    bullets = ["Keep the strongest, verified points from every improved answer",
               "Remove mistakes and resolve disagreements claim by claim",
               "Never follow the majority just because it is the majority",
               "Say plainly what is still uncertain"]
    yy = y - 42
    for b in bullets:
        c.setFillColor(VIOLET)
        c.circle(sx + 22, yy + 3, 2.2, stroke=0, fill=1)
        text(c, b, sx + 32, yy, 10, FONT, INK)
        yy -= 17

    # left bottom note
    para(c, "During step 4 an AI remembers only its first answer. It does not see the other answers or "
            "the reviews it wrote for others, so it cannot be steered by them.",
         lx, 104, 470, 9.5, 13, FONT_SB, TEAL)


def page_numbers(c):
    chrome(c, 6, "Effort and flexibility", "By the numbers: 21 calls, four waits, any panel size",
           "Most calls in each stage run in parallel, so one full round feels like four steps, not twenty-one.")
    # stat tiles
    tiles = [("4", "independent answers", TEAL), ("12", "peer reviews", GOLD),
             ("4", "improved answers", CORAL), ("1", "final synthesis", VIOLET), ("21", "AI calls per question", CYAN)]
    n = len(tiles)
    gap = 14
    tw = (W - 2 * M - gap * (n - 1)) / n
    th = 84
    ty = H - 150 - th
    for i, (num, label, col) in enumerate(tiles):
        x = M + i * (tw + gap)
        box(c, x, ty, tw, th, fill=PANEL, stroke=alpha(col, 0.4), r=14)
        text(c, num, x + 18, ty + th - 44, 30, FONT_B, col)
        text(c, label, x + 18, ty + 18, 10, FONT_SB, INK)
        if i < n - 1 and i != 3:
            text(c, "+", x + tw + gap / 2, ty + th / 2 - 6, 16, FONT_B, MUTED, "center")
        if i == 3:
            text(c, "=", x + tw + gap / 2, ty + th / 2 - 6, 16, FONT_B, MUTED, "center")

    # timeline of waits
    y = ty - 44
    text(c, "WHAT THE USER WAITS FOR", M, y, 9.5, FONT_B, TEAL)
    waves = [("Wait 1", "4 answers, all at once", TEAL), ("Wait 2", "12 reviews, all at once", GOLD),
             ("Wait 3", "4 rewrites, all at once", CORAL), ("Wait 4", "1 synthesis", VIOLET)]
    ww = (W - 2 * M - 3 * 12) / 4
    for i, (t_, s, col) in enumerate(waves):
        x = M + i * (ww + 12)
        box(c, x, y - 52, ww, 38, fill=alpha(col, 0.14), stroke=alpha(col, 0.5), r=8)
        text(c, t_, x + 14, y - 30, 9.5, FONT_B, col)
        text(c, s, x + 14, y - 44, 9, FONT, INK)
        if i < 3:
            arrow(c, x + ww + 1, y - 33, x + ww + 11, y - 33, MUTED, 1.2, 5)

    # flexibility panel
    fy = y - 70
    ph = fy - 62
    box(c, M, 62, (W - 2 * M) / 2 - 8, ph, fill=PANEL, r=12)
    text(c, "BUILT TO FLEX", M + 18, fy - 22, 9.5, FONT_B, CYAN)
    flex = ["Any number of AIs: N answers + N x (N-1) reviews + N rewrites + 1 synthesis",
            "Each seat has its own system prompt, model, provider and settings",
            "Extra review rounds can be added without changing the isolation rule",
            "Panel members can be swapped any time; every run records exactly which models ran"]
    yy = fy - 42
    for b in flex:
        c.setFillColor(CYAN)
        c.circle(M + 22, yy + 3, 2.2, stroke=0, fill=1)
        yy = para(c, b, M + 32, yy, (W - 2 * M) / 2 - 60, 9.8, 13, FONT, INK) - 4

    # stack panel
    sx = M + (W - 2 * M) / 2 + 8
    box(c, sx, 62, (W - 2 * M) / 2 - 8, ph, fill=PANEL, r=12)
    text(c, "RECOMMENDED STARTING STACK", sx + 18, fy - 22, 9.5, FONT_B, GREEN)
    stack = [("Workflow", "Python, LangGraph stateless nodes, fan-out / fan-in"),
             ("Model calls", "Pydantic AI: one library, every major provider, typed outputs"),
             ("Ledger", "PostgreSQL, append-only records for every paper"),
             ("Quality", "Phoenix tracing, Promptfoo regression tests, leak canaries"),
             ("Interface", "Web round-table view showing each stage live")]
    yy = fy - 44
    for k, v in stack:
        text(c, k, sx + 18, yy, 9.5, FONT_B, TEXT)
        text(c, v, sx + 96, yy, 9.5, FONT, INK)
        yy -= 17


def page_why(c):
    chrome(c, 7, "Why it matters", "Why a round table beats a group chat",
           "Open debate lets the loudest model win. Isolated review lets the best evidence win.")
    colw = (W - 2 * M) / 2 - 10
    top = H - 150
    ph = 160
    # left: group chat
    box(c, M, top - ph, colw, ph, fill=PANEL, stroke=alpha(CORAL, 0.4), r=14)
    text(c, "TYPICAL MULTI-AI GROUP CHAT", M + 18, top - 26, 10, FONT_B, CORAL)
    for k, s in enumerate(["The first answer anchors everyone after it",
                           "Models copy each other and a wrong idea snowballs",
                           "Long or confident answers get favoured over correct ones",
                           "Impossible to tell which model contributed what"]):
        text(c, "x", M + 22, top - 52 - k * 22, 11, FONT_B, CORAL)
        text(c, s, M + 38, top - 52 - k * 22, 10.5, FONT, INK)
    # right: roundtable
    rx = M + colw + 20
    box(c, rx, top - ph, colw, ph, fill=PANEL, stroke=alpha(TEAL, 0.5), r=14)
    text(c, "ROUNDTABLE AI", rx + 18, top - 26, 10, FONT_B, TEAL)
    for k, s in enumerate(["Every first answer is genuinely independent",
                           "Reviews are anonymous, fresh and about one answer only",
                           "Each AI improves its own work using criticism, not peer pressure",
                           "Full provenance: every answer, review and rewrite is on record"]):
        text(c, "+", rx + 22, top - 52 - k * 22, 12, FONT_B, TEAL)
        text(c, s, rx + 38, top - 52 - k * 22, 10.5, FONT, INK)

    # next steps
    y = top - ph - 40
    text(c, "PROPOSED NEXT STEPS", M, y, 9.5, FONT_B, GOLD)
    steps = [("1", "Prototype", "4 models, 1 controller, text-only questions. Prove the isolation rule with leak tests."),
             ("2", "Measure", "Run a fixed question set. Compare final answers against each single model."),
             ("3", "Present", "Round-table interface showing every stage live, ready for a wider demo.")]
    sw_ = (W - 2 * M - 2 * 14) / 3
    for i, (n_, t_, b_) in enumerate(steps):
        x = M + i * (sw_ + 14)
        box(c, x, 62, sw_, y - 80, fill=PANEL_2, r=12)
        c.setFillColor(GOLD)
        c.circle(x + 24, y - 36, 12, stroke=0, fill=1)
        text(c, n_, x + 24, y - 40, 11, FONT_B, DARK_TEXT, "center")
        text(c, t_, x + 44, y - 40, 11, FONT_B, GOLD)
        para(c, b_, x + 16, y - 62, sw_ - 32, 10, 14, FONT, INK)
    # back to start link
    text(c, "back to cover", W - M, 50, 8, FONT_SB, MUTED, "right")
    link(c, "cover", (W - M - 70, 44, W - M, 58))


# ---------------------------------------------------------------- build
def build():
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    c = canvas.Canvas(OUT, pagesize=(W, H))
    c.setTitle("Roundtable AI - Executive Concept")
    c.setAuthor("Roundtable AI project")
    c.setSubject("Memory-isolated multi-LLM round-robin review")
    c.showOutline()
    for fn in (page_cover, page_overview, page_table, page_matrix,
               page_memory, page_improve, page_numbers, page_why):
        fn(c)
        c.showPage()
    c.save()
    print("wrote", OUT)


if __name__ == "__main__":
    build()
