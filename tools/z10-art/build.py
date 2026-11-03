"""Write the Z10's art as one SVG in the core protocol's pieces (draft 4).

One-time migration: it took the look the app drew and wrote it into
art/z10.svg, which is now the source; the app draws from that.
It reads what the app had then:

  dom.json                 the page's layout, from extract.py (the old page)
  from/board.svg           the chassis built from Grant's drawing
  from/board-top.svg       for the wells the light shows in
  from/evga-e.svg          the E on the G/PC button
  the fonts the page uses, for the legends as outlines

Every rule of style.css that shapes a cap, a legend or the glass is written
here as SVG with the same numbers, in the drawing's units (1 cqw = 6.49).

Numbering, one count per kind:
  control_1..116       the keys, in reading order; 117 and 118 the slider knobs
  illumination_N       the light key N throws on the deck (pairs with control_N)
  illumination_200+N   key N's light on its own cap
  illumination_400+N   the glow round key N's legend
  legend_1             everything printed on the body
  legend_100+N         what is printed on key N
  body_1, mask_1, screen_1, overlay_1
"""
import json
import math
import os
import re
import xml.etree.ElementTree as ET
from pathlib import Path

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
PUB = ROOT / 'apps' / 'ectomatter' / 'public'
FROM = HERE / 'from'        # the app's old board pictures, kept as this migration's source
OUT = ROOT / 'art' / 'z10.svg'
TIES = HERE / 'ties.json'

D = json.loads((HERE / 'dom.json').read_text())
VB = (6.0, -2.0, 649.0, 260.0)
CQ = D['cqw']                    # 1 cqw in drawing units
PX = D['scale']                  # 1 CSS pixel in drawing units, at the 1240-wide board the look was set at

f = lambda v: f'{v:.3f}'.rstrip('0').rstrip('.') if abs(v) > 1e-9 else '0'

# ---------------------------------------------------------------- fonts
FONTS = {
    'barlow': TTFont(PUB / 'fonts' / 'BarlowSemiCondensed-600-latin.woff2'),
    'dejavu': TTFont('/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf'),
}
CMAPS = {k: v.getBestCmap() for k, v in FONTS.items()}


def glyph_path(ch, x, base, size):
    """The outline of one character at pen position x on the baseline, as a path."""
    face = 'barlow' if ord(ch) in CMAPS['barlow'] else 'dejavu'
    font = FONTS[face]
    name = CMAPS[face].get(ord(ch))
    if name is None:
        raise SystemExit(f'no font has {ch!r}')
    upm = font['head'].unitsPerEm
    s = size / upm
    pen = SVGPathPen(font.getGlyphSet(), ntos=lambda v: f'{v:.2f}'.rstrip('0').rstrip('.'))   # 0.01 of a unit is 0.02 of a pixel
    font.getGlyphSet()[name].draw(TransformPen(pen, (s, 0, 0, -s, x, base)))
    return pen.getCommands()


# ---------------------------------------------------------------- shapes
def rrect(x, y, w, h, rx, ry=None):
    ry = rx if ry is None else ry
    rx, ry = min(rx, w / 2), min(ry, h / 2)
    if rx <= 0 or ry <= 0:
        return f'M{f(x)},{f(y)}h{f(w)}v{f(h)}h{f(-w)}Z'
    return (f'M{f(x + rx)},{f(y)}H{f(x + w - rx)}A{f(rx)},{f(ry)} 0 0 1 {f(x + w)},{f(y + ry)}'
            f'V{f(y + h - ry)}A{f(rx)},{f(ry)} 0 0 1 {f(x + w - rx)},{f(y + h)}'
            f'H{f(x + rx)}A{f(rx)},{f(ry)} 0 0 1 {f(x)},{f(y + h - ry)}'
            f'V{f(y + ry)}A{f(rx)},{f(ry)} 0 0 1 {f(x + rx)},{f(y)}Z')


defs = []
_grads = {}


def grad(kind, stops, key=None):
    """A gradient in the element's own box; stops are (offset, color, opacity)."""
    k = (kind, tuple(stops)) if key is None else key
    if k in _grads:
        return _grads[k]
    gid = f'g{len(_grads) + 1}'
    _grads[k] = gid
    st = ''.join(f'<stop offset="{f(o)}" stop-color="{c}"' + (f' stop-opacity="{f(a)}"' if a != 1 else '') + '/>'
                 for o, c, a in stops)
    if kind == 'v':
        defs.append(f'<linearGradient id="{gid}" x1="0" y1="0" x2="0" y2="1">{st}</linearGradient>')
    elif kind == 'h':
        defs.append(f'<linearGradient id="{gid}" x1="0" y1="0" x2="1" y2="0">{st}</linearGradient>')
    elif kind == 'glow':
        defs.append(f'<radialGradient id="{gid}" cx=".5" cy="0" r="1" '
                    f'gradientTransform="translate(.5 0) scale(1.2 .9) translate(-.5 0)">{st}</radialGradient>')
    return gid


def user_grad(x1, y1, x2, y2, stops):
    gid = f'g{len(_grads) + 1}'
    _grads[('u', gid)] = gid
    st = ''.join(f'<stop offset="{f(o)}" stop-color="{c}"' + (f' stop-opacity="{f(a)}"' if a != 1 else '') + '/>'
                 for o, c, a in stops)
    defs.append(f'<linearGradient id="{gid}" gradientUnits="userSpaceOnUse" x1="{f(x1)}" y1="{f(y1)}" '
                f'x2="{f(x2)}" y2="{f(y2)}">{st}</linearGradient>')
    return gid


_filters = {}


def css_sigma(radius, device=1):
    """The blur WebKit (Skia) gives a CSS shadow of this blur radius, in drawing units: 0.57735 r + 0.5 device
    pixels, drawn at `device` pixels per CSS pixel (caps.png was made at 2, the legends are drawn at 1)."""
    return (0.57735 * radius / PX * device + 0.5) / device * PX


def blur(sigma, dy=0):
    """A Gaussian blur, as CSS draws a shadow of blur radius 2 * sigma."""
    k = ('blur', round(sigma, 4), round(dy, 4))
    if k not in _filters:
        fid = f'f{len(_filters) + 1}'
        _filters[k] = fid
        off = f'<feOffset dy="{f(dy)}"/>' if dy else ''
        defs.append(f'<filter id="{fid}" x="-50%" y="-50%" width="200%" height="200%" '
                    f'color-interpolation-filters="sRGB">{off}<feGaussianBlur stdDeviation="{f(sigma)}"/></filter>')
    return _filters[k]


def inset_shadow(dy, sigma, opacity):
    """CSS's inset box-shadow in black: dark inside the shape where the shape, moved by dy and blurred, is not."""
    k = ('inset', round(dy, 4), round(sigma, 4), opacity)
    if k not in _filters:
        fid = f'f{len(_filters) + 1}'
        _filters[k] = fid
        defs.append(
            f'<filter id="{fid}" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB">'
            f'<feOffset in="SourceAlpha" dy="{f(dy)}" result="o"/>'
            f'<feGaussianBlur in="o" stdDeviation="{f(sigma)}" result="b"/>'
            f'<feComposite in="SourceAlpha" in2="b" operator="out" result="edge"/>'
            f'<feFlood flood-color="#000" flood-opacity="{f(opacity)}"/>'
            f'<feComposite in2="edge" operator="in" result="shade"/>'
            f'<feMerge><feMergeNode in="SourceGraphic"/><feMergeNode in="shade"/></feMerge></filter>')
    return _filters[k]


def glow_mask():
    """.key .glow: radial-gradient(120% 90% at 50% 0%, color, transparent 70%), as a mask, so the light's
    color is the shape's own fill and any renderer can recolor it."""
    if 'glowmask' not in _filters:
        g = grad('glow', [(0, '#fff', 1), (.7, '#fff', 0)])
        _filters['glowmask'] = 'glow'
        defs.append(f'<mask id="glow" maskContentUnits="objectBoundingBox"><rect width="1" height="1" fill="url(#{g})"/></mask>')
    return 'glow'


_clips = []


def clip(d):
    cid = f'c{len(_clips) + 1}'
    _clips.append(cid)
    defs.append(f'<clipPath id="{cid}"><path d="{d}"/></clipPath>')
    return cid


def band(x, y, w, h, r, dx, dy, fill, opacity, spread=0):
    """An inset box-shadow with no blur: the part of the cap the cap moved by (dx, dy), or shrunk by spread, does not cover."""
    outer = rrect(x, y, w, h, r)
    if spread:
        inner = rrect(x + spread, y + spread, w - 2 * spread, h - 2 * spread, max(r - spread, 0))
        return f'<path d="{outer}{inner}" fill-rule="evenodd" fill="{fill}" fill-opacity="{f(opacity)}"/>'
    moved = rrect(x + dx, y + dy, w, h, r)
    cid = clip(outer)
    return (f'<g clip-path="url(#{cid})"><path d="{outer}{moved}" fill-rule="evenodd" fill="{fill}" '
            f'fill-opacity="{f(opacity)}"/></g>')


def bend(k):
    """How tall the keys beside the spacebar are: where its bend goes."""
    x, y, w, h = k['box']
    return next(o['box'][3] for o in D['keys'] if o is not k and abs(o['box'][1] - y) < 1)


# ---------------------------------------------------------------- caps (style.css .key and its variants)
def deck_shade(cy):
    """The deck's shade at a height in the drawing, as board.svg draws it: 42 behind the back row to 37
    behind the front."""
    return min(42, max(37, 42 - (cy - 60) * 5 / 170))


LIN = lambda v: (v / 255) / 12.92 if v / 255 <= 0.04045 else ((v / 255 + 0.055) / 1.055) ** 2.4
ENC = lambda l: max(0, min(255, round(255 * (l * 12.92 if l <= 0.0031308 else 1.055 * l ** (1 / 2.4) - 0.055))))
GRAY = lambda v: '#' + f'{v:02x}' * 3
# EVGA's photo, every one-unit key averaged: across the top, the light (relative to the key's mean) is
# 0.969 + 0.502 (u - 0.5) + 0.596 (u - 0.5)^2, the dish of a slightly concave top as the photo's light
# shows it; down the key it is even
DISH = (0.969, 0.502, 0.596)
# A cap is a base, its whole place, with sloped sides up to a smaller top: the photo's key cells averaged
# put the top at 0.684 of the key spacing across and 0.75 down, centered, and its sides in deep shadow,
# 5% of the top's light. At the 26-unit spacing that insets the top 3.11 units at each side and 2.25 at
# the back and front
INSET_X, INSET_Y = round((24 - 0.684 * 26) / 2, 2), round((24 - 0.75 * 26) / 2, 2)
SIDES = 0.05


def dish_stops(mean):
    a, t, c = DISH
    m = sum(a + t * (u / 20 - .5) + c * (u / 20 - .5) ** 2 for u in range(21)) / 21
    return [(u / 10, GRAY(ENC(LIN(mean) * (a + t * (u / 10 - .5) + c * (u / 10 - .5) ** 2) / m)), 1) for u in range(11)]


# ---------------------------------------------------------------- caps (style.css .key and its variants)
def space_base(k, x, y, w, h, r):
    """The spacebar's base where it runs past the bottom of its well, over the chassis ledge, as the Z10's
    is: its walls stop partway down there, so from above its sides are half as wide, stepping in level with the
    bottom of the keys beside it, the step's outer corner round. None where it stays in its well."""
    well = next((wb for wb in WELLS if wb[0] < x + w / 2 < wb[2] and wb[1] < y + h / 2 < wb[3]), None)
    if not well or y + h <= well[3]:
        return None
    sy = max(o['box'][1] + o['box'][3] for o in D['keys'] if o is not k and abs(o['box'][1] - y) < 2)
    # (its front wall there 0.93 mm wide, as Grant chose it)
    ix, by = INSET_X / 2, y + h - INSET_Y / 2 + 0.93 * 1.38
    x0, x1 = x + ix, x + w - ix
    return (f'M{f(x + r)},{f(y)}H{f(x + w - r)}A{f(r)},{f(r)} 0 0 1 {f(x + w)},{f(y + r)}'
            f'V{f(sy - ix)}A{f(ix)},{f(ix)} 0 0 1 {f(x1)},{f(sy)}V{f(by - r)}A{f(r)},{f(r)} 0 0 1 {f(x1 - r)},{f(by)}'
            f'H{f(x0 + r)}A{f(r)},{f(r)} 0 0 1 {f(x0)},{f(by - r)}V{f(sy)}A{f(ix)},{f(ix)} 0 0 1 {f(x)},{f(sy - ix)}'
            f'V{f(y + r)}A{f(r)},{f(r)} 0 0 1 {f(x + r)},{f(y)}Z')


def cap(k):
    x, y, w, h = k['box']
    cls = k['cls'].split()
    out = []
    if 'molded' in cls:
        r = 0.13 * CQ
        out.append(f'<path d="{rrect(x, y, w, h, r)}" fill="url(#{grad("v", [(0, "#232323", 1), (1, "#191919", 1)])})"/>')
        out.append(band(x, y, w, h, r, 0, PX, '#fff', .12))
        out.append(band(x, y, w, h, r, 0, -PX, '#000', .7))
        out.append(band(x, y, w, h, r, 0, 0, '#fff', .03, spread=PX))
        return out
    # a cap as EVGA's photo has it: its base the whole place, its sides in deep shadow, and a smaller,
    # slightly concave top as light as the deck around it on average; no outline
    r = 0.24 * CQ
    shade = round(deck_shade(y + h / 2))
    base = rrect(x, y, w, h, r)
    if k['label'] == 'Space':
        base = space_base(k, x, y, w, h, r) or base
    out.append(f'<path d="{base}" fill="#000" fill-opacity=".8" filter="url(#{blur(0.08 * CQ, 0.08 * CQ)})"/>')
    out.append(f'<path d="{base}" fill="{GRAY(ENC(LIN(shade) * SIDES))}"/>')
    tx, ty, tw = x + INSET_X, y + INSET_Y, w - 2 * INSET_X
    if k['label'] == 'Space':
        # the spacebar: its top inset like every key's and as light as the deck, flat as in the photo;
        # a soft bend where the tops of the keys beside it end, and its lip, 72% of the top's light, down
        # to the front of its base
        p = (bend(k) - INSET_Y) / h
        d = 0.23 * CQ / h
        top, lip = GRAY(shade), GRAY(ENC(LIN(shade) * 0.72))
        th = y + h - INSET_Y * 0.5 - ty
        pp = (y + p * h - ty) / th
        shape_t = rrect(tx, ty, tw, th, r)
        # its top has the same slightly concave texture as every key's top; the lip fades in over it
        out.append(f'<path d="{shape_t}" fill="url(#{grad("h", dish_stops(shade))})"/>')
        dd = d * h / th
        vs = [(0, lip, 0), (round(pp - dd, 5), lip, 0), (round(pp + dd, 5), lip, 1), (1, lip, 1)]
        out.append(f'<path d="{shape_t}" fill="url(#{grad("v", vs)})"/>')
        return out
    th = h - 2 * INSET_Y
    out.append(f'<path d="{rrect(tx, ty, tw, th, r)}" fill="url(#{grad("h", dish_stops(shade))})"/>')
    out.extend(bump(k['label'], tx, ty, tw, th, shade))
    return out


def bump(label, tx, ty, tw, th, shade):
    """The homing marks EVGA's photo shows: a short raised bar low on F and J (a third of the top's width,
    centered, at 0.76 of its height) and a raised dot in the middle of the numpad's 5. Raised, so each
    catches the light along its top and throws a little shadow below."""
    if label in ('F', 'J'):
        bw, bh = 0.33 * tw, 0.075 * th
        bx, by = tx + (tw - bw) / 2, ty + 0.76 * th - bh / 2
        rx = bh / 2
    elif label == 'Keypad5':
        bw = bh = 0.11 * tw
        bx, by = tx + (tw - bw) / 2, ty + 0.56 * th - bh / 2
        rx = bh / 2
    else:
        return []
    lit = GRAY(ENC(min(1, LIN(shade) * 2.4)))
    body = GRAY(ENC(LIN(shade) * 1.3))
    return [
        f'<path d="{rrect(bx, by + PX, bw, bh, rx)}" fill="#000" fill-opacity=".28" filter="url(#{blur(0.5 * PX)})"/>',
        f'<path d="{rrect(bx, by, bw, bh, rx)}" fill="{body}"/>',
        f'<path d="{rrect(bx + rx * 0.3, by, bw - rx * 0.6, bh * 0.5, min(rx, bh * 0.25))}" fill="{lit}"/>',
    ]


# ---------------------------------------------------------------- legends
MUTE = ('<path d="M3 9h4l5-4v14l-5-4H3z"/>'
        '<path d="M16 9l6 6M22 9l-6 6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>')
E_SVG = ET.parse(FROM / 'evga-e.svg').getroot()


def legend_shapes(k, with_opacity=True):
    """What is printed on a cap: its characters as outlines, its icons and its E."""
    out = []
    for run in k['leg']:
        size = run['size'] * run['scale']
        d = ''.join(glyph_path(c['ch'], c['box'][0], run['base'], size) for c in run['chars'] if c['ch'].strip())
        if d:
            op = f' fill-opacity="{f(run["opacity"])}"' if with_opacity and run['opacity'] != 1 else ''
            out.append(f'<path d="{d}"{op}/>')
    for ic in k['icons']:
        x, y, w, h = ic['box']
        s = min(w, h) / 24
        op = f' opacity="{f(ic["opacity"])}"' if with_opacity and ic['opacity'] != 1 else ''
        out.append(f'<g transform="translate({f(x + (w - 24 * s) / 2)} {f(y + (h - 24 * s) / 2)}) scale({f(s)})"{op}>{MUTE}</g>')
    for im in k['imgs']:
        x, y, w, h = im['box']
        vw, vh = map(float, E_SVG.get('viewBox').split()[2:])
        s = min(w / vw, h / vh)
        paths = ''.join(f'<path d="{p.get("d")}" fill="{p.get("fill")}"/>' for p in E_SVG.iter('{http://www.w3.org/2000/svg}path'))
        out.append(f'<g transform="translate({f(x + (w - vw * s) / 2)} {f(y + (h - vh * s) / 2)}) scale({f(s)})">{paths}</g>')
    return out


def print_shapes(p):
    out = []
    for run in p['runs']:
        d = ''.join(glyph_path(c['ch'], c['box'][0], run['base'], run['size']) for c in run['chars'] if c['ch'].strip())
        if d:
            out.append(f'<path d="{d}" fill="#9c9c9c"/>')
    return out


# ---------------------------------------------------------------- the body, from board.svg
SVGNS = '{http://www.w3.org/2000/svg}'
ET.register_namespace('', 'http://www.w3.org/2000/svg')
board = ET.parse(FROM / 'board.svg').getroot()
body, printed = [], []
for el in board:
    if el.tag == SVGNS + 'defs':
        for g in el:
            defs.append(ET.tostring(g, encoding='unicode').replace(' xmlns="http://www.w3.org/2000/svg"', ''))
        continue
    s = ET.tostring(el, encoding='unicode').replace(' xmlns="http://www.w3.org/2000/svg"', '')
    # what the drawing prints on the body (in its print gray) is a legend
    (printed if '#9c9c9c' in s else body).append(s)
# The app laid a copy of the deck, with the wells cut out, over the light; along each well's edge that copy
# covered about half a pixel of the well. WELL_INSET draws each well that much smaller, so the edge sits
# where the approved look has it.
WELL_INSET = float(os.environ.get('WELL_INSET', '0.5')) * PX


def inset_well(el):
    if not el.tag.endswith('rect') or WELL_INSET == 0:
        return el
    for a, d in (('x', 1), ('y', 1), ('width', -2), ('height', -2)):
        el.set(a, f(float(el.get(a)) + d * WELL_INSET))
    if el.get('rx'):
        el.set('rx', f(max(float(el.get('rx')) - WELL_INSET, 0)))
    return el


body = [ET.tostring(inset_well(ET.fromstring(b)), encoding='unicode').replace(' xmlns="http://www.w3.org/2000/svg"', '')
        if b.startswith('<rect') and 'fill="#000"' in b and 'rx="3.78"' in b else b for b in body]
top = ET.parse(FROM / 'board-top.svg').getroot()
holes = [ET.tostring(inset_well(r), encoding='unicode').replace(' xmlns="http://www.w3.org/2000/svg"', '').replace(' fill="#000"', '')
         for r in top.find(SVGNS + 'mask') if r.get('fill') == '#000']
defs.append(f'<clipPath id="mask_1">{"".join(holes)}</clipPath>')

# ---------------------------------------------------------------- number the keys in reading order
keys = D['keys']
rows = []
for k in sorted(keys, key=lambda k: (k['box'][1], k['box'][0])):
    if rows and abs(rows[-1][0]['box'][1] - k['box'][1]) < 2:
        rows[-1].append(k)
    else:
        rows.append([k])
order = [k for row in rows for k in sorted(row, key=lambda k: k['box'][0])]
lit = lambda k: 'nolight' not in k['cls'].split()

halos, controls, lights, legends, ties = [], [], [], [], {'controls': [], 'screens': []}
import os
sigma = 0.009 * VB[2] * float(os.environ.get('HALO_SIGMA', '1'))   # drawHalos: blurred by 0.9% of the board's width
for n, k in enumerate(order, 1):
    x, y, w, h = k['box']
    tie = {'pos': k['pos'], 'label': k['label'], 'control': n}
    controls.append(f'<g id="control_{n}">' + ''.join(cap(k)) + '</g>')
    if lit(k):
        hx, hy, hw, hh = x - 0.08 * w, y - 0.12 * h, 1.16 * w, 1.24 * h
        halos.append(f'<g id="illumination_{n}" clip-path="url(#mask_1)"><path d="{rrect(hx, hy, hw, hh, 0.28 * hw, 0.28 * hh)}" '
                     f'fill="#fff" fill-opacity="{os.environ.get('HALO_OP', '.3')}" filter="url(#{blur(sigma)})"/></g>')
        lights.append(f'<g id="illumination_{200 + n}" opacity=".07"><path d="{rrect(x + INSET_X, y + INSET_Y, w - 2 * INSET_X, h - 2 * INSET_Y, 0.24 * CQ)}" '
                      f'fill="#fff" mask="url(#{glow_mask()})"/></g>')
        tie['illumination'] = [n, 200 + n, 400 + n]
    shapes = legend_shapes(k)
    if shapes:
        # the print itself sits in an inner group, so the glow round it can reuse it: a <use> takes the
        # color of where it is used, and the glow takes the print's fading too, as a text-shadow does
        legends.append(f'<g id="legend_{100 + n}" fill="#9e9e9e" color="#9e9e9e"><g id="print_{n}">' + ''.join(shapes) + '</g></g>')
        tie['legend'] = 100 + n
        if lit(k):
            lights.append(f'<g id="illumination_{400 + n}" fill="#fff" color="#fff" filter="url(#{blur(0.11 * CQ)})">'
                          f'<use href="#print_{n}"/></g>')
    ties['controls'].append(tie)

# the slider knobs (.board .knob): corners 14% / 28% round, the drawing's knob shading
for i, (x, y, w, h) in enumerate(D['knobs']):
    n = len(order) + 1 + i
    controls.append(f'<g id="control_{n}"><path d="{rrect(x, y, w, h, 0.14 * w, 0.28 * h)}" '
                    f'fill="url(#{grad("v", [(0, "#dcdcdc", 1), (1, "#a2a2a2", 1)])})"/></g>')
    ties['controls'].append({'label': ['Dimmer', 'Volume'][i], 'control': n})

# the deck's own print: the drawing's, and the slider and G/PC labels
deck_print = printed + [s for p in D['prints'] for s in print_shapes(p)]

# the display, and the glass over it and its bezel (.board .glass)
px_, py_, pw_, ph_ = D['panel']
screen = f'<rect id="screen_1" x="{f(px_)}" y="{f(py_)}" width="{f(pw_)}" height="{f(ph_)}" fill="#080a0c"/>'
gx, gy, gw, gh = D['glass']
t = math.radians(112)
dx, dy = math.sin(t), -math.cos(t)
L = abs(gw * dx) + abs(gh * dy)
cx, cy = gx + gw / 2, gy + gh / 2
sheen = user_grad(cx - dx * L / 2, cy - dy * L / 2, cx + dx * L / 2, cy + dy * L / 2,
                  [(0, '#fff', .075), (.34, '#fff', .035), (.344, '#fff', 0), (1, '#fff', 0)])
fall = grad('v', [(0, '#fff', .06), (.3, '#fff', 0)])
overlay = (f'<g id="overlay_1"><rect x="{f(gx)}" y="{f(gy)}" width="{f(gw)}" height="{f(gh)}" fill="url(#{fall})"/>'
           f'<rect x="{f(gx)}" y="{f(gy)}" width="{f(gw)}" height="{f(gh)}" fill="url(#{sheen})"/>'
           f'<rect x="{f(gx)}" y="{f(gy)}" width="{f(gw)}" height="{f(PX)}" fill="#fff" fill-opacity=".14"/></g>')
ties['screens'].append({'label': 'Display', 'screen': 1, 'overlay': 1})

layer = lambda name, items: f'<g id="{name}" inkscape:groupmode="layer" inkscape:label="{name}">\n' + '\n'.join(items) + '\n</g>'
svg = '\n'.join([
    f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" '
    f'viewBox="{f(VB[0])} {f(VB[1])} {f(VB[2])} {f(VB[3])}" width="1240" height="{f(1240 * VB[3] / VB[2])}">',
    '<!-- EVGA Z10 RGB, drawn by Grant Garrison. Pieces follow the Ectomatter core protocol, draft 4. -->',
    '<defs>', *defs, '</defs>',
    layer('body', [f'<g id="body_1">{"".join(body)}</g>']),
    layer('illumination-under', halos),
    layer('legend-body', [f'<g id="legend_1">{"".join(deck_print)}</g>']),
    layer('control', controls),
    layer('illumination-over', lights),
    layer('legend', legends),
    layer('screen', [screen]),
    layer('overlay', [overlay]),
    '</svg>',
])
OUT.parent.mkdir(parents=True, exist_ok=True)
OUT.write_text(svg + '\n')
TIES.write_text(json.dumps(ties, indent=1) + '\n')
# The device description is written by hand (device.json, JSON with comments, the core
# protocol's draft 6): each control names its piece of the drawing by number, and its legend and lights
# follow from it by rule. Check that the drawing's numbering is still the one it names.
from core import read  # noqa: E402
named = {c['label']: c['art'] for c in read(OUT.parent.parent / 'device.json')['controls']}
for t in ties['controls']:
    if named.get(t['label']) != t['control']:
        print(f"device.json names {t['label']} {named.get(t['label'])}, but the drawing has it as control_{t['control']}")
print(f'wrote {OUT.relative_to(ROOT)} ({len(svg) // 1024} KB), {len(order)} keys, {len(D["knobs"])} knobs')
