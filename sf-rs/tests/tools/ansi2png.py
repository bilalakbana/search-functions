"""Render `tmux capture-pane -e -p` output (ANSI SGR) to a PNG, Ghostty's Catppuccin Macchiato palette.
usage: python3 -I ansi2png.py in.ansi out.png"""
import re, sys
from PIL import Image, ImageDraw, ImageFont

PAL = ['#494d64', '#ed8796', '#a6da95', '#eed49f', '#8aadf4', '#f5bde6', '#8bd5ca', '#a5adcb',
       '#5b6078', '#ec7486', '#8ccf7f', '#e1c682', '#78a1f6', '#f2a9dd', '#63cbc0', '#b8c0e0']
BG, FG = '#24273a', '#cad3f5'
FONT = '/usr/local/share/fonts/i/IntelOneMono_{}.ttf'

def hexrgb(h): return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))
def c256(n):
    if n < 16: return hexrgb(PAL[n])
    if n < 232:
        n -= 16; v = [0, 95, 135, 175, 215, 255]
        return (v[n // 36], v[(n // 6) % 6], v[n % 6])
    g = 8 + (n - 232) * 10; return (g, g, g)

def parse(text):
    rows = []
    st = dict(fg=None, bg=None, bold=False, dim=False, italic=False, rev=False)
    for line in text.split('\n'):
        cells = []
        for tok in re.split(r'(\x1b\[[0-9;:]*m)', line):
            m = re.fullmatch(r'\x1b\[([0-9;:]*)m', tok)
            if not m:
                for ch in tok: cells.append((ch, dict(st)))
                continue
            ps = [int(x) if x else 0 for x in re.split('[;:]', m.group(1))] or [0]
            i = 0
            while i < len(ps):
                p = ps[i]
                if p == 0: st.update(fg=None, bg=None, bold=False, dim=False, italic=False, rev=False)
                elif p == 1: st['bold'] = True
                elif p == 2: st['dim'] = True
                elif p == 3: st['italic'] = True
                elif p == 7: st['rev'] = True
                elif p == 22: st['bold'] = st['dim'] = False
                elif p == 23: st['italic'] = False
                elif p == 27: st['rev'] = False
                elif 30 <= p <= 37: st['fg'] = hexrgb(PAL[p - 30])
                elif 90 <= p <= 97: st['fg'] = hexrgb(PAL[p - 90 + 8])
                elif 40 <= p <= 47: st['bg'] = hexrgb(PAL[p - 40])
                elif 100 <= p <= 107: st['bg'] = hexrgb(PAL[p - 100 + 8])
                elif p == 39: st['fg'] = None
                elif p == 49: st['bg'] = None
                elif p in (38, 48) and i + 1 < len(ps):
                    key = 'fg' if p == 38 else 'bg'
                    if ps[i + 1] == 2 and i + 4 < len(ps): st[key] = tuple(ps[i + 2:i + 5]); i += 4
                    elif ps[i + 1] == 5 and i + 2 < len(ps): st[key] = c256(ps[i + 2]); i += 2
                i += 1
        rows.append(cells)
    return rows

def render(rows, out, size=17):
    reg, bold = ImageFont.truetype(FONT.format('Regular'), size), ImageFont.truetype(FONT.format('Bold'), size)
    ita = ImageFont.truetype(FONT.format('Italic'), size)
    cw, ch = int(reg.getlength('M')), int(size * 1.45)
    cols = max(len(r) for r in rows)
    img = Image.new('RGB', (cols * cw + 16, len(rows) * ch + 16), hexrgb(BG))
    d = ImageDraw.Draw(img)
    for y, row in enumerate(rows):
        for x, (c, s) in enumerate(row):
            fg, bg = s['fg'] or hexrgb(FG), s['bg']
            if s['rev']: fg, bg = (bg or hexrgb(BG)), fg
            if s['dim']: fg = tuple(int(a * 0.6 + b * 0.4) for a, b in zip(fg, hexrgb(BG)))
            px, py = 8 + x * cw, 8 + y * ch
            if bg: d.rectangle([px, py, px + cw, py + ch], fill=bg)
            if c != ' ': d.text((px, py + (ch - size) // 2), c, font=bold if s['bold'] else ita if s['italic'] else reg, fill=fg)
    img.save(out)

render(parse(open(sys.argv[1], encoding='utf-8', errors='replace').read()), sys.argv[2])
