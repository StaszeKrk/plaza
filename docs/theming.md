# Theming Plaza

Plaza's appearance is two independent parts that you mix freely:

- a **palette**: the colors, and
- a **skin**: everything non-color, that is the border style and corner radius,
  the glyph/icon set, and how selections and source badges are drawn.

Both are plain TOML files. Plaza ships built-in presets and reads your own files
from `~/.config/plaza/palettes/` and `~/.config/plaza/skins/`.

## Selecting a palette and skin

Press `o` for the options menu. The `Palette` and `Skin` rows cycle through every
palette and skin Plaza knows about (built-ins plus your files), and the active
choice is written to `~/.config/plaza/settings.json`:

```json
{
  "palette": "nord",
  "skin": "soft"
}
```

You can also set those two fields by hand.

## Adding your own

Put a file in the matching directory; the file name (without `.toml`) is the
theme name:

```
~/.config/plaza/palettes/my-theme.toml
~/.config/plaza/skins/my-shape.toml
```

- New files are picked up the next time Plaza starts.
- While Plaza is running, edits to the palette or skin that is currently active
  reload automatically, within about a third of a second. This makes editing a
  theme a live preview: keep Plaza open next to the file.
- A file may set only the fields it wants to change. Anything omitted falls back
  to the default (`plaza-dusk` for palettes, `soft` for skins).
- An unknown key is an error (so a typo is caught rather than silently ignored).

A quick way to start is to copy one of the built-in presets from the source tree
(`src/theme/palettes/` or `src/theme/skins/`) and edit it.

## Colors

Anywhere a palette expects a color you can write:

| Form | Example | Notes |
| --- | --- | --- |
| hex | `"#7aa2f7"` or `"#abc"` | true color (24-bit) |
| ANSI name | `"cyan"`, `"darkgray"`, `"lightblue"` | uses the terminal's own palette, so it tracks the terminal theme |
| 256-color index | `"99"` or `"color99"` | the xterm 256 palette |
| `"reset"` | `"reset"` | the terminal's default foreground |
| `"none"` | `"none"` | background only: do not paint a background, keep the terminal's |

ANSI names: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `gray`,
`darkgray`, `white`, and the `light*` variants (`lightred`, `lightgreen`,
`lightyellow`, `lightblue`, `lightmagenta`, `lightcyan`).

## Palette format

Every field is a color. A partial file inherits the rest from `plaza-dusk`.

| Field | Used for |
| --- | --- |
| `bg` | window background (`"none"` keeps the terminal's) |
| `fg` | primary text |
| `muted` | secondary text: versions, hints, status line |
| `accent` | primary accent: active borders, cursor, wordmark |
| `title` | panel titles |
| `section` | sidebar section headers |
| `border_idle` | a panel's border when it is neither focused nor hovered |
| `border_hover` | the hovered panel's border (navigate mode) |
| `border_active` | the focused panel's border (interact mode) |
| `highlight_fg` | selected row text (and chip-badge text) |
| `highlight_bg` | selected row background |
| `badge_repo` | the pacman/repo source badge |
| `badge_aur` | the AUR source badge |
| `badge_official` | the collapsed `[official]` badge |
| `installed` | the installed check mark and version |
| `update` | the update arrow and the new version |
| `success` | a finished action |
| `warning` | a running action, and the AUR recency warning |
| `danger` | a failed action, removals, and out-of-date markers |

Example (`~/.config/plaza/palettes/example.toml`):

```toml
bg            = "#161821"
fg            = "#c8ccd4"
muted         = "#6b7089"
accent        = "#7aa2f7"
border_active = "#7aa2f7"
border_hover  = "#e0af68"
badge_repo    = "#7aa2f7"
badge_aur     = "#bb9af7"
installed     = "#9ece6a"
update        = "#e0af68"
danger        = "#f7768e"
# any field left out inherits from plaza-dusk
```

## Skin format

A partial file inherits the rest from `soft`.

```toml
highlight = "bar"      # bar | reversed | bold
badge     = "chip"     # brackets | bare | chip

[border]
thickness = 1          # 0 none, 1 light, 2 heavy
radius    = 1          # 0 square, 1 rounded (only at thickness 1)
double    = false      # two-line border; overrides the weight

[icons]
enabled  = true        # false: use plain unicode and hide decorative glyphs
package  = ""
repo     = ""
aur      = ""
installed = ""
update   = ""
running  = ""
success  = ""
fail     = ""
cursor   = "▸"
lock     = ""
search   = ""
```

### Borders

`thickness` and `radius` are numbers, but the terminal can only draw the
box-drawing glyphs that exist, so the useful range is small and honest:

- `thickness`: `0` no border, `1` light (`─`), `2` heavy (`━`).
- `radius`: `0` square corners (`┌`), `1` rounded (`╭`). Rounded only exists at
  `thickness = 1`; heavy and double borders are always square because Unicode has
  no heavy or double arc corner.
- `double = true` draws a two-line border (`═`) and supersedes the weight.

There is no pixel sizing: a terminal is a grid of characters, not pixels. For any
combination the numbers cannot express, set the six glyphs yourself, which wins
over everything above:

```toml
[border.glyphs]
top_left     = "╭"
top_right    = "╮"
bottom_left  = "╰"
bottom_right = "╯"
horizontal   = "─"
vertical     = "│"
```

### Highlight and badge

These are named modes, not amounts:

- `highlight`: how the selected list row is drawn.
  - `bar`: a filled accent bar (`highlight_bg` behind `highlight_fg`).
  - `reversed`: reverse video.
  - `bold`: bold accent text, no fill.
- `badge`: how source badges are drawn.
  - `brackets`: `[aur]`.
  - `bare`: `aur`.
  - `chip`: ` aur ` filled with the badge color.

### Icons

`enabled = false` switches to portable unicode (`✓ ✗ ▸ ↑ ◐`) and drops the
decorative glyphs (package, lock, search). Use it for terminals without a Nerd
Font. When enabled, each glyph is a string: paste the character directly or use a
TOML `"\uXXXX"` escape.

## Built-in presets

Palettes: `plaza-dusk` (default), `gruvbox`, `nord`, `dracula`, `tokyo-night`,
`solarized-dark`, `ansi`.

Skins: `soft` (default: light rounded, nerd icons, bar highlight, chip badges),
`sharp` (heavy square, bold highlight, bracket badges), `plain` (light square,
icons off, reversed highlight, bracket badges).

The `ansi` palette is special: it is written with ANSI color names, so it adopts
your terminal's own 16 colors and follows whatever theme the terminal is set to.
