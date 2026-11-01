# Sentinal update package (desktop + mobile)

One folder for the full glass redesign. Hand it to your coding agent, and edit the specs when you want to change something.

```
START_HERE.md                  master prompt: paste this first
prompts/1-desktop.md           desktop steps (island, prompt, sheets, dashboard, session)
prompts/2-mobile.md            mobile steps (island, prompt pill, built-in keyboard, ^C confirm)
specs/GLASS_SPEC.md            tokens, glass recipe, desktop layouts, prompt grammar
specs/MOBILE_GLASS_SPEC.md     phone shell + keyboard layers, key codes, behaviour
design/                        visual references (.dc.html), open with: npx serve design
CHANGELOG.md                   record your edits here
```

## How to update
| You want to change… | Edit |
|---|---|
| Colours, glass blur, radius | `specs/GLASS_SPEC.md` §1 and §3 |
| Island contents or ticker priority | `specs/GLASS_SPEC.md` §4 (desktop), `specs/MOBILE_GLASS_SPEC.md` §1 (phone) |
| Prompt commands (`go`, `run`, `find`…) | `specs/GLASS_SPEC.md` §5 |
| Dashboard sections | `specs/GLASS_SPEC.md` §7, `specs/MOBILE_GLASS_SPEC.md` §2 |
| Keyboard keys, snippets, Ctrl combos | `specs/MOBILE_GLASS_SPEC.md` §4 |
| ^C confirm threshold | `specs/MOBILE_GLASS_SPEC.md` §5 |

After editing, add a line to `CHANGELOG.md` and tell the agent: "Apply the changes in CHANGELOG.md since <date>."
