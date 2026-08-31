# Themes

Six named themes plus the built-in dark. Each lives in its own file under
`src/lib/styles/themes/` and is selected by `data-theme` on the document root,
set from `settings.appearance.theme`.

## The contract

`tokens.css` defines the complete set on `:root` — that is the dark theme and
the fallback. A theme file overrides ONLY the colour tokens, inside a single
`:root[data-theme="<name>"]` block. It must not touch radii, type, motion,
spacing, blur or the chevron mask: those are structural and shared, and a theme
that changes them stops being a theme.

**Every theme must define all 35 of these.** Leaving one out silently inherits
the dark value, which is how a light theme ends up with white text on white.

```
surfaces   --bg-0 --bg-1 --surface-1 --surface-2 --surface-3 --surface-4
           --swatch-empty --window-wash
overlays   --overlay-1 --scrim-1 --scrim-2
borders    --border-1 --border-2 --border-3 --window-border
           --border-media-1 --border-media-2
text       --text-1 --text-2 --text-3 --text-bright
accent     --accent --on-accent
ext hues   --ext-1 --ext-2 --ext-3 --ext-4 --ext-5 --ext-6 --ext-7
status     --danger --ok --warn
shadows    --shadow-1 --shadow-2 --shadow-3
```

`--accent-soft`, `--accent-strong` and `--danger-soft` are derived with
`color-mix` from the two above them and must NOT be redefined.

## Rules that make a theme usable rather than merely coloured

- **`color-scheme`.** A light theme must set `color-scheme: light` in its block,
  or the scrollbars and form controls stay dark against it.
- **`--on-accent` is the text drawn ON the accent** — the "right now in buffer"
  badge uses it. Pick it for contrast against your accent, not for harmony.
- **Contrast.** `--text-1` on `--bg-0` needs at least 7:1, `--text-2` at least
  4.5:1. These are read at a glance over a grid of thumbnails.
- **The scrims sit over user content**, not over your background: a photo can be
  any colour, so a scrim must stay dark enough to carry white glyphs whatever is
  underneath, even in a light theme.
- **The seven `--ext-*` hues** label file types and must be distinguishable from
  each other AND from `--accent`, which marks the live clipboard item.
- **Shadows** on a light background need to be softer and less opaque than on a
  dark one; copying the dark values makes a light theme look bruised.

## Adding one

1. Write `src/lib/styles/themes/<name>.css` with one
   `:root[data-theme="<name>"]` block.
2. Import it in `src/lib/styles/tokens.css` at the bottom.
3. Add the name to `THEMES` in `src/lib/types.ts`.

Nothing else needs to change: the store applies `data-theme`, and every
component already reads tokens.
