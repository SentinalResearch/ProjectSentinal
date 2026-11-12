"""Build docs/guide/out/sentinal-slides.pptx from the brand PowerPoint template.

Each new slide is a copy of one template slide (so masters, fonts, colours and
the slide number stay attached); its text is replaced, and columns or diagrams
are added in the template's own styles. Content mirrors sentinal-slides.tex.

Usage: python make_pptx.py <template.pptx> <out.pptx>   (needs python-pptx)
"""
import copy
import sys

from pptx import Presentation
from pptx.dml.color import RGBColor
from pptx.enum.shapes import MSO_CONNECTOR, MSO_SHAPE
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
from pptx.oxml.ns import qn
from pptx.util import Emu, Inches, Pt

INK, TEXT2, TEXT3, LINE = "F2F2F0", "A6A6A2", "8A8A86", "262626"
BLUE, BLUE_TEXT, TERM = "3D5AFE", "8C9EFF", "0C0C0C"
STATES = {
    "CREATED": "B9A8FF", "RUNNING": "29D35F", "WAITING": "FFD000", "IDLE": "9C9C98",
    "STALLED": "FF8A00", "COMPLETED": "36C5E8", "FAILED": "FF3B2F", "INTERRUPTED": "FF5CA8",
}
SANS, MONO = "Archivo", "JetBrains Mono"
LEFT, WIDTH = 0.8, 11.73

src = Presentation(sys.argv[1])
TEMPLATE = list(src.slides)
TITLE, AGENDA, SECTION, CONTENT, CODE, STATE, NUMBER, CLOSING = range(8)


def clone(index):
    """A new slide that is a copy of template slide `index`, pictures included."""
    proto = TEMPLATE[index]
    slide = src.slides.add_slide(proto.slide_layout)
    for shape in list(slide.shapes):
        shape._element.getparent().remove(shape._element)
    for element in proto.shapes._spTree.iterchildren():
        if element.tag in (qn("p:nvGrpSpPr"), qn("p:grpSpPr")):
            continue
        new = copy.deepcopy(element)
        for blip in new.iter(qn("a:blip")):
            rel = proto.part.rels[blip.get(qn("r:embed"))]
            blip.set(qn("r:embed"), slide.part.relate_to(rel.target_part, rel.reltype))
        slide.shapes._spTree.append(new)
    return slide


def shape(slide, name):
    return next(s for s in slide.shapes if s.name == name)


def set_lines(sh, lines):
    """Replace a text shape's paragraphs, keeping the first paragraph's style."""
    body = sh.text_frame._txBody
    paragraphs = body.findall(qn("a:p"))
    proto = paragraphs[0]
    for p in paragraphs:
        body.remove(p)
    for line in lines:
        p = copy.deepcopy(proto)
        runs = p.findall(qn("a:r"))
        for r in runs[1:]:
            p.remove(r)
        runs[0].find(qn("a:t")).text = line
        body.append(p)


def drop(slide, name):
    el = shape(slide, name)._element
    el.getparent().remove(el)


def text(slide, x, y, w, h, value, size=16, color=INK, font=SANS, bold=False, anchor=MSO_ANCHOR.TOP):
    box = slide.shapes.add_textbox(Inches(x), Inches(y), Inches(w), Inches(h))
    tf = box.text_frame
    tf.word_wrap = True
    tf.vertical_anchor = anchor
    for side in ("margin_left", "margin_right", "margin_top", "margin_bottom"):
        setattr(tf, side, 0)
    for i, line in enumerate(value if isinstance(value, list) else [value]):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        run = p.add_run()
        run.text = line
        run.font.size, run.font.name, run.font.bold = Pt(size), font, bold
        run.font.color.rgb = RGBColor.from_string(color)
        p.space_after = Pt(4)
    return box


def rule(slide, x, y, w, color=INK, weight=1.0):
    line = slide.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(x), Inches(y), Inches(x + w), Inches(y))
    line.line.color.rgb = RGBColor.from_string(color)
    line.line.width = Pt(weight)
    return line


def box(slide, x, y, w, h, label, accent=False, size=14):
    b = slide.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(x), Inches(y), Inches(w), Inches(h))
    b.fill.background()
    b.line.color.rgb = RGBColor.from_string(BLUE if accent else TEXT2)
    b.line.width = Pt(2 if accent else 0.75)
    b.shadow.inherit = False
    tf = b.text_frame
    tf.word_wrap = True
    tf.vertical_anchor = MSO_ANCHOR.MIDDLE
    tf.margin_left = tf.margin_right = Inches(0.15)
    lines = label if isinstance(label, list) else [label]
    for i, line in enumerate(lines):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.alignment = PP_ALIGN.LEFT
        run = p.add_run()
        run.text = line
        run.font.size, run.font.name = Pt(size if i == 0 else size - 2), MONO if i == 0 else SANS
        run.font.color.rgb = RGBColor.from_string(INK if i == 0 else TEXT2)
    return b


def arrow(slide, x1, y1, x2, y2):
    c = slide.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(x1), Inches(y1), Inches(x2), Inches(y2))
    c.line.color.rgb = RGBColor.from_string(TEXT2)
    c.line.width = Pt(1)
    tail = c.line._get_or_add_ln()
    end = tail.makeelement(qn("a:tailEnd"), {"type": "triangle", "w": "med", "len": "med"})
    tail.append(end)


def content(title, subtitle=None):
    slide = clone(CONTENT)
    set_lines(shape(slide, "Text 0"), [title])
    if subtitle:
        set_lines(shape(slide, "Text 1"), [subtitle])
    else:
        drop(slide, "Text 1")
    drop(slide, "Text 2")
    return slide


def columns(slide, items, y=2.5, size=18):
    gap = 0.4
    w = (WIDTH - gap * (len(items) - 1)) / len(items)
    for i, (label, body) in enumerate(items):
        x = LEFT + i * (w + gap)
        rule(slide, x, y, w)
        text(slide, x, y + 0.2, w, 0.35, label, size=13, color=BLUE_TEXT, font=MONO)
        text(slide, x, y + 0.7, w, 2.6, body, size=size)


def footnote(slide, value, y=5.9):
    text(slide, LEFT, y, WIDTH, 0.4, value, size=14, color=TEXT2)


def section(number, name):
    slide = clone(SECTION)
    set_lines(shape(slide, "Text 0"), [f"Section {number}"])
    set_lines(shape(slide, "Text 1"), [name])


# 1 Title
s = clone(TITLE)
set_lines(shape(s, "Text 0"), ["Sentinal"])
set_lines(shape(s, "Text 1"), ["Every terminal on your machines, watched and answered from anywhere"])
set_lines(shape(s, "Text 2"), ["Sohith Vishnu Sai"])
set_lines(shape(s, "Text 3"), ["Sentinal Research · 8 November 2026"])

# 2 Agenda
s = clone(AGENDA)
set_lines(shape(s, "Text 1"), [
    "What Sentinal is",
    "The watcher: progress without changing the program",
    "Daily use",
    "The development cycle",
    "What comes next",
])

section(1, "What Sentinal is")

s = content("Long work lives in a terminal you sit at", "Sentinal moves it into Sessions the daemon owns")
columns(s, [
    ("SEE", "What is running, waiting or failed on every Machine."),
    ("ANSWER", "A push when a run fails or waits. Answer an agent's question from the dashboard."),
    ("DRIVE", "Open the live terminal on a phone and type into it."),
])
footnote(s, "Local-first: a daemon per Machine, data in SQLite on that Machine, traffic only inside Tailscale.")

s = content("One control, many workers", "Workers own every Session; the control routes and summarises")
for i, name in enumerate(["Phone or browser", "AI agents", "Push service"]):
    box(s, LEFT + i * 4.0, 2.2, 3.6, 0.7, name)
box(s, LEFT, 3.4, 11.6, 0.9, ["CONTROL", "web app · login · read model · routing · Agent API · push"], accent=True, size=16)
for i in range(3):
    box(s, LEFT + i * 4.0, 4.8, 3.6, 0.7, "Worker")
    cx = LEFT + i * 4.0 + 1.8
    if i < 2:
        arrow(s, cx, 2.9, cx, 3.4)
    else:
        arrow(s, cx, 3.4, cx, 2.9)
    arrow(s, cx, 4.3, cx, 4.8)
footnote(s, "The read model holds no terminal text. No daemon listens on 0.0.0.0.")

s = clone(STATE)
set_lines(shape(s, "Text 0"), ["Every Session has one state"])
badges = [n for n in s.shapes if n.name.startswith("Text ") and n.name != "Text 0"]
for el in badges:
    el._element.getparent().remove(el._element)
proto = badges[0]._element
for i, (name, color) in enumerate(STATES.items()):
    el = copy.deepcopy(proto)
    s.shapes._spTree.append(el)
    badge = s.shapes[-1]
    badge.left = Inches(LEFT + (i % 4) * 2.2)
    badge.top = Inches(1.9 + (i // 4) * 0.7)
    badge.fill.solid()
    badge.fill.fore_color.rgb = RGBColor.from_string(color)
    set_lines(badge, [name])
columns(s, [
    ("WAITING", "A prompt or an agent's question needs an answer."),
    ("STALLED", "Quiet for ten minutes while it should be working."),
], y=3.8)
footnote(s, "States are never colour-only: the label is always there.", y=6.0)

s = content("Four ways work enters Sentinal", "From full terminals to observing a process you already started")
columns(s, [
    ("FULL", ["sentinal watch", "A persistent terminal: output, screen, input."]),
    ("COMMANDS", ["sentinal hook", "Commands from ordinary terminals."]),
    ("AUTO", ["sentinal auto on", "Every new terminal starts inside a Session."]),
    ("PROCESS", ["sentinal adopt", "Metrics and exit of a running PID. Read-only."]),
], size=17)

section(2, "The watcher")

s = content("Output becomes Events, with no model", "Deterministic: the same input always gives the same Events")
columns(s, [
    ("1 FRAME", "Bytes to bounded lines, escapes removed."),
    ("2 OBSERVE", "Lines, Commands, processes, metrics, screens."),
    ("3 ADAPT", "Generic, training, pytest, cargo, servers, agents."),
    ("4 LIMIT", "Progress at most every 5 s."),
    ("5 HEALTH", "Healthy, warning, stalled or failed, with evidence."),
], size=16)

s = content("Sentinal reads the agent's screen", "Read as lines, questions were garbled, missed, or ended early")
columns(s, [
    ("DETECT", "A numbered menu of 2 to 6 options, and the question above it."),
    ("END", "When the menu leaves the screen. No time limit."),
    ("WORKS WITH", "Any program with a numbered menu: Claude Code, Codex, Vibe, Antigravity and more."),
    ("ANSWER", "Click or tap an option. The input is audit-logged."),
], size=17)

s = clone(CODE)
set_lines(shape(s, "Text 0"), ["Progress without changing the program"])
code = shape(s, "Text 1")
code.top, code.height = Inches(2.05), Inches(1.85)
set_lines(code, [
    "Evaluating:  42%|####      | 21/50   -> 21 of 50 item",
    "[4/10] building                      -> 4 of 10 item",
    "Processing sample 12 of 200          -> 12 of 200 sample",
    "accuracy 41/50 on condition A        -> nothing",
])
for r in code.text_frame._txBody.iter(qn("a:rPr")):
    r.set("sz", "1500")
text(s, LEFT, 4.2, WIDTH, 0.35, "NEXT", size=12, color=BLUE_TEXT, font=MONO)
text(s, LEFT, 4.55, WIDTH, 0.4, "Programs being written now can say it themselves, in one printed line:", size=16)
line = copy.deepcopy(code._element)
s.shapes._spTree.append(line)
line = s.shapes[-1]
line.top, line.height = Inches(5.1), Inches(0.75)
set_lines(line, ["::sentinal progress current=3 total=50 unit=task"])
for r in line.text_frame._txBody.iter(qn("a:rPr")):
    r.set("sz", "1500")
text(s, LEFT, 1.45, WIDTH, 0.4, "Explicit counters only: a bare fraction is never progress", size=18, color=TEXT2)

section(3, "Daily use")

s = content("Finished Sessions no longer pile up", "Hide, delete, or clear a whole Machine")
columns(s, [
    ("HIDE", "Leaves every list on every device. History stays searchable; restore from the Hidden page."),
    ("DELETE", "Output, Commands, Events, metrics and search rows erased. Logged; IDs never reused."),
    ("CLEAR FINISHED", "One action on the Machine page hides all of its finished Sessions."),
])

s = content("Navigation that takes no space", "The page name drops down the main places")
columns(s, [
    ("PLACES", "Overview, Terminals, Machines, History, Recipes, Settings."),
    ("OPENS", "On hover on a desktop, on a tap on a phone, with Enter from the keyboard. Each place shows its prompt word."),
])

section(4, "Development cycle")

s = content("Development never touches production", "After a test daemon replaced this Mac's Sessions on the control, 5 November")
columns(s, [
    ("JUST DEV", "Own data, config, ports 5273 and 7520. A DEV badge."),
    ("ISOLATED", "Refuses production paths. Never links, never updates itself."),
    ("ONE INSTALL", "Install ID and build commit in every heartbeat; the control refuses a second live install."),
])

s = content("From dev to a release", "Each step is a gate; nothing is pushed until the owner asks")
steps = [("dev", "test first"), ("just check", "test-e2e"), ("just dev", "by hand"),
         ("just promote", "into main"), ("just ship", "owner only")]
w, gap = 2.0, 0.42
for i, (cmd, note) in enumerate(steps):
    x = LEFT + i * (w + gap)
    box(s, x, 2.8, w, 1.1, [cmd, note], accent=i == len(steps) - 1, size=14)
    if i:
        arrow(s, x - gap, 3.35, x, 3.35)
footnote(s, "Update the control first, then the workers, when a release changes the protocol.", y=4.6)

section(5, "What comes next")

s = clone(AGENDA)
set_lines(shape(s, "Text 0"), ["Next, in order"])
body = shape(s, "Text 1")
set_lines(body, [
    "Retest screen questions with real Claude Code, Vibe and Antigravity",
    "Progress API: ::sentinal progress and ::sentinal metric",
    "Rules: metrics from any program, full-screen apps, builds, servers",
    "Release v2.11.0",
    "Keepers, so a daemon restart no longer ends a multi-day run",
    "Data collection, then swappable local models, then a fine-tuned model",
])
for r in body.text_frame._txBody.iter(qn("a:rPr")):
    r.set("sz", "2000")
text(s, LEFT, 6.2, WIDTH, 0.4, "The full list, with a prompt for coding agents, is in docs/TASKS.md", size=14, color=TEXT2)

s = clone(NUMBER)
set_lines(shape(s, "Text 0"), ["The size of it today"])
set_lines(shape(s, "Text 1"), ["1,100"])
set_lines(shape(s, "Text 2"), ["Rust tests · 81 web test files · 17 browser scenarios"])
footnote(s, "90,000 lines of Rust · 31,000 lines of TypeScript · 5 Machines in daily use", y=5.0)

s = clone(CLOSING)

# Remove the eight template slides, keeping only the new ones.
ids = src.slides._sldIdLst
for sld_id in list(ids)[:8]:
    src.part.drop_rel(sld_id.rId)
    ids.remove(sld_id)

src.save(sys.argv[2])
print("slides:", len(src.slides))
