# Sentinal — Glass mobile + terminal keyboard spec (< 768px)

Reference: `design/Sentinal Glass Mobile.dc.html` (M1–M6). Tokens and the glass recipe are shared with desktop; see `GLASS_SPEC.md` §1–3. This replaces the top bar + menu from the earlier `MOBILE_SPEC.md`.

## 1. Shell
- **Island (top):** 46px glass pill, 16px side insets, top = safe area + 7px. Contents: logo · page name · one live ticker item (highest priority) · yellow needs-you count. Tap the count to filter the dashboard; tap the ticker item to open that session.
- **Prompt pill (bottom):** 56px glass pill, 16px insets, bottom = safe area + 30px. Placeholder: `❯ go, run, find, y / n…`. Tap to open the prompt sheet (M2).
- No tab bar and no sidebar. Detail screens put a ‹ back button inside the island; swiping from the left edge also goes back.
- Content pads 112px at the top and 110px at the bottom so nothing sits under the glass.

## 2. Dashboard (M1)
"N need you." hero 44/42. The WAITING card is full width, with the prompt well and y / n buttons (52 × 48). Other attention cards sit two across. Then one glass lane per machine with 44px session rows; offline machines at 50 % opacity.

## 3. Prompt sheet (M2)
Suggestions panel (glass, 52px rows, the selected row tinted with a 3px accent bar), then the input pill (accent edge, Cancel), then the **built-in keyboard, letters layer, without the accessory row**. Same grammar as desktop (`go`, `y|n`, `run`, `find`, `split`, `kill`).

## 4. Built-in terminal keyboard (M3–M5)
Replaces the system keyboard in the terminal and the prompt. Set `inputmode="none"` on xterm's hidden textarea (and the prompt input) so the OS keyboard never opens; render our own panel and write bytes to the PTY directly.

**Panel:** background `rgba(22,22,26,.96)`, 1px top edge, padding `6px 4px` + safe area, 7px row gap, 5px key gap. Keys have radius 8, mono font, and a background of `rgba(255,255,255,.11)`. Modifier keys use `.05` plus a 1px edge.

**Rows (top to bottom)**
1. **Chip strip** (30px pills, scrolls sideways): context suggestions. y / n when the session is WAITING, recent commands (`↑ python train.py …`, accent tint), `clear`, `exit`. On the symbols layer: tokens from recent output (`loss`, `epoch`, `CUDA`, `error`). Tap inserts the text; it never sends Enter.
2. **Accessory row** (always visible, 38px): `Ctrl` `Alt` `Esc` `Tab` `←` `↑` `↓` `→`.
3. **Layer** (see below).
4. **Bottom row:** layer switch (`#+=` / `abc`) · `|` · `~` · `/` · `space` (wide; horizontal swipe moves the cursor) · `-` · `⏎` (accent).

**Layers**
- **Letters (default):** number row 1–0 with shifted symbols as small labels (long-press or ⇧ to type them) · qwertyuiop · asdfghjkl · ⇧ zxcvbnm ⌫. Lowercase by default; ⇧ is one-shot, double-tap locks it.
- **Symbols:** `- _ = + [ ] { } \ |` · `~ \` ; : ' " , . < >` · **snippet row** (accent tint): `&&` `||` `$(` `~/` `../` `2>&1` `| grep` + ⌫ · `! @ # $ % ^ & * ?`.
- **Ctrl (latched):** a 4×3 grid of 50px keys, each with a name: ^C interrupt (red) · ^D EOF / exit · ^Z suspend · ^L clear · ^R search hist · ^A line start · ^E line end · ^U delete line · ^W delete word · ^K kill to end · ^P previous · ^N next. Hint: "or tap any letter for Ctrl + letter".

**Behaviour**
- Ctrl and Alt: tap = latch for the next key (accent fill, "latched" sub-label); double-tap = lock; tap again = off.
- Repeat on hold for ⌫, arrows and space-swipe (400ms delay, then 40ms).
- Light haptic on key press (`navigator.vibrate(5)` where supported).
- No autocorrect, autocapitalise or predictive text. No key-press popups above keys (they cover the terminal).
- Key codes: arrows `\x1b[A/B/C/D`, Esc `\x1b`, Tab `\t`, Enter `\r`, Ctrl+X = `charCode & 0x1f`, Alt+X = `\x1b` + X.
- Header button `⌨︎` hides or shows the panel; when hidden, the terminal gets the full height and the island shows a "keyboard" chip to bring it back.
- An external Bluetooth keyboard hides the panel automatically, showing the accessory row only.
- Height: about 330px with the chip strip, so the terminal keeps at least 40 % of the screen on a 390 × 844 phone.

## 5. ^C confirm (M6)
On sessions running ≥ 30s, `^C` opens a glass sheet: title, what it interrupts, runtime, last checkpoint, a **hold-to-send** destructive button (600ms hold, fill progress) and Cancel. Under 30s, ^C goes through immediately.

## 6. Rules kept
States are never colour-only; commands monospace; touch targets ≥ 44pt (key rows 38–50px tall, full-width hit area including gaps); terminal surface solid `#0C0C0C`, never glass; respect `prefers-reduced-motion`.
