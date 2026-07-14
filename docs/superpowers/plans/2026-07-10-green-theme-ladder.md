# Green Theme Ladder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `green-light` and `green-lightest` reader themes mirroring the sepia ladder, and insert both into the Alt+t cycle after kindle-green.

**Architecture:** Pure data change: two new JSON theme objects in the themes repo derived from kindle-green by the same per-channel blend-toward-white factors the sepia ladder applies to kindle-sepia, plus cycle-list edits in linux-lit's compiled default and the two stored configs. No loader changes (theme.rs reads entries generically).

**Tech Stack:** JSON (`~/utono/themes/.config/themes/themes-unified.json`, its own git repo), Rust (`src/config.rs` default cycle), Python one-liner for color math.

## Global Constraints

- Design doc: `docs/superpowers/specs/2026-07-10-green-theme-ladder-design.md`.
- Cycle insert position: immediately AFTER `kindle-green`.
- Default theme stays `kindle-sepia` (compiled) / whatever configs hold — do not change `DEFAULT_THEME`.
- Config files must be edited while NO linux-lit instance runs (`pgrep -af linux-lit` first).
- Keys constant in the sepia ladder stay constant in the green ladder.

---

### Task 1: Generate and add the two theme JSON entries

**Files:**
- Modify: `~/utono/themes/.config/themes/themes-unified.json` (kindle-green at ~line 4165-region)

**Interfaces:**
- Produces: JSON entries named `"green-light"`, `"green-lightest"` with sections `meta`, `dwl`, `kitty`, `firefox`, `nvim`, `linux-lit` (same shape as `sepia-light`/`sepia-lightest`).

- [ ] **Step 1: Extract the four reference themes and compute blend factors**

For every key whose value differs between `kindle-sepia` and `sepia-light` (resp. `sepia-lightest`), compute the per-channel blend factor toward white, then apply the same factor to kindle-green's value for that key:

```bash
cd /tmp/claude-1000/-home-mlj-utono-linux-lit/*/scratchpad
python3 - <<'EOF'
import json, re, copy
p = '/home/mlj/utono/themes/.config/themes/themes-unified.json'
d = json.load(open(p))
ks, sl, sx, kg = d['kindle-sepia'], d['sepia-light'], d['sepia-lightest'], d['kindle-green']

def hex2rgb(h): h=h.lstrip('#'); return tuple(int(h[i:i+2],16) for i in (0,2,4))
def rgb2hex(r): return '#%02x%02x%02x' % r
def blend_t(old, new):
    # average per-channel t where new = old + t*(255-old)
    ts = [(n-o)/(255-o) if o < 255 else 0 for o,n in zip(hex2rgb(old), hex2rgb(new))]
    return sum(ts)/3
def lighten(base, t):
    return rgb2hex(tuple(round(c + t*(255-c)) for c in hex2rgb(base)))

HEX = re.compile(r'^#[0-9a-fA-F]{6}$')
def derive(sep_variant):
    out = copy.deepcopy(kg)
    def walk(a, b, g):   # a=kindle-sepia node, b=sepia-variant node, g=green node
        for k in b:
            if k not in a or k not in g: continue
            if isinstance(b[k], dict): walk(a[k], b[k], g[k])
            elif isinstance(b[k], str) and isinstance(a[k], str) and a[k] != b[k]:
                if HEX.match(a[k]) and HEX.match(b[k]) and HEX.match(str(g[k])):
                    g[k] = lighten(g[k], blend_t(a[k], b[k]))
                else:
                    g[k] = b[k]  # non-hex changed value (e.g. rgba alpha): take variant's
    walk(ks, sep_variant, out)
    return out

gl = derive(sl); gx = derive(sx)
# dwl: mirror the sepia variants' stripped block, keep green focuscolor
for t in (gl, gx):
    t['dwl'] = {'rootcolor': '#08526b', 'focuscolor': kg['dwl']['focuscolor']}
# linux-lit: cursor/karaoke alphas mirror the ladder using kindle-green's cursor RGB
m = re.match(r'rgba\((\d+), (\d+), (\d+),', kg['linux-lit']['cursor_line_bg'])
r,g_,b = m.groups()
gl['linux-lit']['cursor_line_bg']      = f'rgba({r}, {g_}, {b}, 0.12)'
gl['linux-lit']['phrase_highlight_bg'] = f'rgba({r}, {g_}, {b}, 0.18)'
gx['linux-lit']['cursor_line_bg']      = f'rgba({r}, {g_}, {b}, 0.10)'
gx['linux-lit']['phrase_highlight_bg'] = f'rgba({r}, {g_}, {b}, 0.14)'
# meta: names/labels
for t, name in ((gl,'green-light'), (gx,'green-lightest')):
    if 'meta' in t and isinstance(t['meta'], dict):
        for mk in ('name','label','title'):
            if mk in t['meta']:
                t['meta'][mk] = name if mk=='name' else name.replace('-',' ').title()
# insert right after kindle-green preserving order
new = {}
for k,v in d.items():
    new[k]=v
    if k=='kindle-green':
        new['green-light']=gl; new['green-lightest']=gx
json.dump(new, open(p,'w'), indent=2)
print('OK', list(new)[list(new).index('kindle-green'):list(new).index('kindle-green')+3])
EOF
```

Expected: `OK ['kindle-green', 'green-light', 'green-lightest']`

- [ ] **Step 2: Validate the JSON and eyeball the new entries**

```bash
jq -e '."green-light"."linux-lit".phrase_highlight_bg, ."green-lightest"."linux-lit".cursor_line_bg, ."green-light".dwl' ~/utono/themes/.config/themes/themes-unified.json
```

Expected: `"rgba(58, 75, 67, 0.18)"`, `"rgba(58, 75, 67, 0.10)"`, `{"rootcolor": "#08526b", "focuscolor": "#4f7a5c"}`. Also `jq 'keys' | rg green` shows both names, and `git -C ~/utono/themes diff --stat` shows only themes-unified.json.

- [ ] **Step 3: Commit (themes repo)**

```bash
cd ~/utono/themes && git add .config/themes/themes-unified.json && git commit -m "feat: green-light + green-lightest reader themes (mirror sepia ladder)"
```

### Task 2: Cycle wiring in linux-lit

**Files:**
- Modify: `src/config.rs:236-238` (`default_theme_cycle`)
- Modify: `~/.config/linux-lit/config.json`, `~/.config/linux-lit/config-dev.json` (`theme_cycle`)

**Interfaces:**
- Consumes: theme names `green-light`, `green-lightest` from Task 1.

- [ ] **Step 1: Insert into the compiled default cycle**

In `src/config.rs` `default_theme_cycle()`, change the array to:

```rust
    ["kindle-sepia", "kindle-green", "green-light", "green-lightest", "zenbones-light", "zenwritten-light"]
```

(keep the surrounding `.map`/`.to_vec()` shape unchanged).

- [ ] **Step 2: Build + existing config tests**

Run: `cargo build 2>&1 | rg -c error` → expect no errors; `cargo test --bins config 2>&1 | rg "test result"` → all pass.

- [ ] **Step 3: Edit the stored configs (no instance running)**

```bash
pgrep -af "linux-lit" || echo NO_INSTANCE
# only proceed on NO_INSTANCE; else stop and report
for f in ~/.config/linux-lit/config.json ~/.config/linux-lit/config-dev.json; do
  python3 - "$f" <<'EOF'
import json,sys
p=sys.argv[1]; d=json.load(open(p))
c=d.get('theme_cycle',[])
for n in ('green-lightest','green-light'):
    if n in c: c.remove(n)
c[c.index('kindle-green')+1:c.index('kindle-green')+1]=['green-light','green-lightest']
d['theme_cycle']=c; json.dump(d,open(p,'w'),indent=2); print(p,c)
EOF
done
```

Expected: both files print a cycle containing `..., "kindle-green", "green-light", "green-lightest", "zenbones-light", ...`.

- [ ] **Step 4: Commit (linux-lit)**

```bash
cd ~/utono/linux-lit && git add src/config.rs && git commit -m "feat: green-light/green-lightest in default theme cycle"
```

### Task 3: Headless visual verification + to-do checkoff

**Files:**
- Modify: `docs/to-do/to-do.md` (mark the green-themes item `[X]`)

- [ ] **Step 1: Launch headless pinned to green-lightest and screenshot**

Set `"theme": "green-lightest"` in `config-dev.json` (still no instance running), then per CLAUDE.md Headless Verification:

```bash
cd ~/utono/linux-lit && LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NO_MPV=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman XDG_RUNTIME_DIR=/run/user/1000 \
  dbus-run-session cage -- ./target/debug/linux-lit 2>/tmp/cage-green.log &
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200; sleep 6
grim /tmp/green-lightest.png && stat -c%s /tmp/green-lightest.png
wtype -M alt -k t -m alt && sleep 2 && grim /tmp/green-next.png   # cycles: verify Alt+t traverses the greens
```

Read both PNGs: green-lightest shows a near-white green-tinted card; the Alt+t shot shows the next cycle entry. Then `pkill -f "cage -- ./target/debug/linux-lit"` and restore the previous `"theme"` value in config-dev.json.

- [ ] **Step 2: Mark to-do + commit**

Put `[X]` at the start of the green-themes item's first line in `docs/to-do/to-do.md`, then:

```bash
git add docs/to-do/to-do.md && git commit -m "docs: mark green theme ladder to-do done"
```
