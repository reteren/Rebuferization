import colorsys, io

def hex_of(h, s, l):
    r, g, b = colorsys.hls_to_rgb(h/360.0, l, s)
    return '#%02x%02x%02x' % (round(r*255), round(g*255), round(b*255))

def lum(hx):
    hx = hx.lstrip('#')
    c = [int(hx[i:i+2], 16)/255 for i in (0, 2, 4)]
    f = lambda v: v/12.92 if v <= 0.03928 else ((v+0.055)/1.055)**2.4
    return 0.2126*f(c[0]) + 0.7152*f(c[1]) + 0.0722*f(c[2])

def ratio(a, b):
    la, lb = lum(a), lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi+0.05)/(lo+0.05)

def rgba(hx, a):
    hx = hx.lstrip('#')
    return 'rgba(%d, %d, %d, %s)' % (int(hx[0:2],16), int(hx[2:4],16), int(hx[4:6],16), a)

def build(name, *, hue, sat, dark, bg_l, text_hex, accent, on_accent, ext, danger, ok, warn,
          scheme, comment, tint_hex):
    """Surfaces climb in even lightness steps from bg-0 so cards separate by a
    consistent amount; borders and overlays are the text colour at low alpha,
    which keeps them tinted with the theme instead of neutral grey."""
    L = bg_l
    step = 0.030 if dark else -0.022
    surf = [hex_of(hue, sat, max(0.0, min(1.0, L + step*i))) for i in range(1, 6)]
    veil = text_hex if dark else tint_hex
    shadow_base = hex_of(hue, min(sat*1.4, 0.6), 0.03 if dark else 0.35)
    sa = (0.45, 0.55, 0.65) if dark else (0.10, 0.14, 0.18)
    lines = [
        ('color-scheme', 'dark' if dark else 'light'),
        ('bg-0', hex_of(hue, sat, L)),
        ('bg-1', rgba(hex_of(hue, sat, L + step*0.6), '0.72')),
        ('surface-1', rgba(surf[0], '0.78')),
        ('surface-2', rgba(surf[1], '0.82')),
        ('surface-3', rgba(surf[2], '0.92')),
        ('surface-4', rgba(surf[3], '0.95')),
        ('swatch-empty', surf[4]),
        ('window-wash',
         'radial-gradient(120%% 100%% at 50%% 0%%, %s, transparent 70%%)' % rgba(accent, '0.14')),
        ('overlay-1', rgba(veil, '0.07' if dark else '0.05')),
        # Scrims lie over the user's own images, so they stay near-neutral: a
        # tinted scrim would colour their photos.
        ('scrim-1', 'rgba(10, 9, 8, 0.58)'),
        ('scrim-2', 'rgba(12, 11, 10, 0.58)'),
        ('border-1', rgba(veil, '0.08' if dark else '0.10')),
        ('border-2', rgba(veil, '0.15' if dark else '0.17')),
        ('border-3', rgba(veil, '0.24' if dark else '0.28')),
        ('window-border', rgba(veil, '0.10' if dark else '0.12')),
        ('border-media-1', 'rgba(255, 255, 255, 0.14)'),
        ('border-media-2', 'rgba(255, 255, 255, 0.18)'),
        ('text-1', text_hex),
        ('text-2', rgba(text_hex, '0.64' if dark else '0.72')),
        ('text-3', rgba(text_hex, '0.40' if dark else '0.48')),
        ('text-bright', '#ffffff'),
        ('accent', accent),
        ('on-accent', on_accent),
    ]
    for i, e in enumerate(ext, 1):
        lines.append(('ext-%d' % i, e))
    lines += [
        ('danger', danger), ('ok', ok), ('warn', warn),
        ('shadow-1', '0 1px 2px %s' % rgba(shadow_base, str(sa[0]))),
        ('shadow-2', '0 10px 30px %s' % rgba(shadow_base, str(sa[1]))),
        ('shadow-3', '0 18px 60px %s' % rgba(shadow_base, str(sa[2]))),
    ]
    body = '\n'.join('  --%s: %s;' % (k, v) if k != 'color-scheme'
                     else '  color-scheme: %s;\n' % v for k, v in lines)
    css = '/* Theme: %s\n   %s */\n:root[data-theme="%s"],\n[data-theme="%s"] {\n%s\n}\n' % (
        name, comment, name, name, body)
    io.open('src/lib/styles/themes/%s.css' % name, 'w', encoding='utf-8', newline='\n').write(css)
    bg = hex_of(hue, sat, L)
    t2 = None
    # composite text-2 over bg to measure it honestly
    a = 0.64 if dark else 0.72
    tb, bb = text_hex.lstrip('#'), bg.lstrip('#')
    t2 = '#%02x%02x%02x' % tuple(
        round(int(tb[i:i+2],16)*a + int(bb[i:i+2],16)*(1-a)) for i in (0,2,4))
    return name, ratio(text_hex, bg), ratio(t2, bg), ratio(on_accent, accent)

results = []

# 1. EMBER — the warm dark nobody has. Every existing dark is cool or neutral;
#    this one is charcoal with a red-brown undertone and an amber accent, which
#    is the palette that stays comfortable late at night.
results.append(build('ember', hue=22, sat=0.20, dark=True, bg_l=0.055,
    text_hex='#f5ece2', tint_hex='#f5ece2',
    accent='#ff9e5e', on_accent='#1a0d05',
    ext=['#ffb86c', '#8fd6a0', '#c9a6ff', '#ffd479', '#ff7a90', '#6fd8e0', '#b8d96a'],
    danger='#ff7a90', ok='#7fd6a4', warn='#ffcf6b', scheme='dark',
    comment='Warm dark: charcoal with a red-brown undertone and an amber accent.\n   The only warm dark in the set, and the easiest on the eyes at night.'))

# 2. PAPER — the warm light. Both existing lights are cool off-whites; this is
#    ivory with warm grey ink, the way a page reads rather than a screen.
results.append(build('paper', hue=38, sat=0.34, dark=False, bg_l=0.955,
    text_hex='#2a2119', tint_hex='#2a2119',
    accent='#954a18', on_accent='#ffffff',
    ext=['#1f6feb', '#177d5c', '#7b3fbf', '#a8630c', '#c0304a', '#0f7b93', '#4a7a17'],
    danger='#c0304a', ok='#177d5c', warn='#a8630c', scheme='light',
    comment='Warm light: ivory ground with warm grey ink and a burnt-sienna\n   accent. Reads like paper rather than a screen, which is the point.'))

# 3. OCEAN — deep teal-navy, the gap between darkblue and dark-green. Surfaces
#    carry real colour rather than a tint, and the accent is a bright aqua.
results.append(build('ocean', hue=196, sat=0.38, dark=True, bg_l=0.075,
    text_hex='#e2f2f7', tint_hex='#e2f2f7',
    accent='#3ad6d6', on_accent='#03191c',
    ext=['#7ab8ff', '#4ee0b8', '#b79cff', '#ffc46b', '#ff7d9c', '#57d8f0', '#9ede63'],
    danger='#ff7d9c', ok='#4ee0b8', warn='#ffc46b', scheme='dark',
    comment='Deep teal-navy — the gap between the blue and the green themes.\n   Its surfaces carry real colour rather than a tint of one.'))

# 4. WINE — deep burgundy. Distinct from dark-purple, which is a cool violet:
#    this is warm and red-leaning, with a rose accent.
results.append(build('wine', hue=345, sat=0.26, dark=True, bg_l=0.070,
    text_hex='#f7e8ec', tint_hex='#f7e8ec',
    accent='#ff86a8', on_accent='#22060e',
    ext=['#8fb4ff', '#5ed0ae', '#d79cff', '#ffc178', '#ff6f8d', '#63cfe8', '#a8d96a'],
    danger='#ff6f8d', ok='#5ed0ae', warn='#ffc178', scheme='dark',
    comment='Deep burgundy with a rose accent. Warm and red-leaning, where the\n   purple theme is a cool violet — the two never read as the same room.'))

print(f"{'тема':<8} {'text-1':>8} {'text-2':>8} {'on-accent':>10}")
for n, a, b, c in results:
    flag = 'OK' if (a >= 7 and b >= 4.5 and c >= 4.5) else 'ПРОВЕРИТЬ'
    print(f"{n:<8} {a:>7.1f}: {b:>7.1f}: {c:>9.1f}:  {flag}")
