"""The Z10 core's print: what each key's cap has printed on it, as words, and
the typefaces its art's legends are drawn in, for Ectomatter to set the
legends anew in another typeface, size or place (the Key lab's Legends
section).

  print.py fonts          the core's fonts/ folder: the art's typeface, and
                          its signs the typeface lacks cut from DejaVu Sans Bold
  print.py device FILE    each control's "print" (and "centered") and the
                          core's own "print" written into a device file, in place

The words come from the page the art was drawn from (dom.json): each key's
runs, their part of the legend by their class (main, top, sub), and the
signs the art sets larger (scale). The art sets the legends 1 unit lower
than the page laid them out; the measures below are the art's.
"""
import json
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORE = ROOT
DOM = json.load(open(Path(__file__).with_name('dom.json')))
FONTS = CORE / 'fonts'
DEJAVU = Path('/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf')
DEJAVU_LICENSE = Path('/usr/share/licenses/dejavu-sans-fonts/LICENSE')
OFL = Path('/usr/share/licenses/aajohan-comfortaa-fonts/OFL.txt')
# the signs the art draws from DejaVu Sans Bold, which Barlow lacks
SIGNS = '▲◀▶▼⊞≡'
# the keys whose legend the Z10 prints in the middle of the cap
CENTERD = {35, 44, 66, 78, 83, 94, 101}
ROLES = {'main': None, 'top': 'shift', 'sub': 'second'}

OWN = '''  // How the drawing's legends are printed, for setting them anew from each
  // key's "print" (the Key lab's Legends section): the typeface they are
  // drawn in, and the one for the signs it lacks (both in fonts/); how tall
  // a legend of one letter or sign is and a legend of words (a capital's
  // height); how far down from the top edge of a cap's top a one-letter
  // legend starts; and the icons, each drawn on a 24-unit square, "size"
  // times as tall as its capitals would be and "drop" times them below
  // its line.
  "print": {
    "typeface": "Barlow Semi Condensed", "weight": 600, "symbols": "Z10 Symbols",
    "fonts": ["fonts/BarlowSemiCondensed-SemiBold.ttf", "fonts/Z10Symbols-Bold.ttf"],
    "letters": 3.786, "words": 2.568, "inset": 2.022,
    "icons": { "mute": { "fill": "M3 9h4l5-4v14l-5-4H3z", "stroke": "M16 9l6 6M22 9l-6 6", "width": 2, "size": 1.92, "drop": 0.15 } }
  },
'''

HEADER_OLD = '''  // <400 + art>. "lit": false is a key with no LED of its own.'''
HEADER_NEW = '''  // <400 + art>. "lit": false is a key with no LED of its own.
  // "print" is what its cap has printed on it, a part at a time: its own
  // legend, its shifted sign beside it ("shift") and its second function
  // at its foot ("second"), each its words or one of the icons above;
  // "scale" a sign printed larger than the rest. "centered": true is a key
  // whose legend sits in the middle of its cap.'''


def runs_of(key):
    """A key's print, as the device file writes it: its parts in order."""
    out = []
    for r in key['leg']:
        if not r['text'].strip():
            continue
        run = {'text': r['text']}
        role = ROLES[r['cls'].split()[0]]
        if role:
            run['role'] = role
        if r['scale'] != 1:
            run['scale'] = r['scale']
        out.append(run)
    for ic in key['icons']:
        out.append({'icon': ic['cls'].split()[-1], 'role': ROLES[ic['cls'].split()[0]]})
    return out


def compact(v):
    """JSON on one line, with a space inside each brace, as the file writes it."""
    if isinstance(v, dict):
        return '{ ' + ', '.join(f'{json.dumps(k)}: {compact(x)}' for k, x in v.items()) + ' }'
    if isinstance(v, list):
        return '[' + ', '.join(compact(x) for x in v) + ']'
    return json.dumps(v, ensure_ascii=False)


def cut_at_ours(line):
    """A control's line without the fields this writes, which come last."""
    for field in (', "centered":', ', "print":'):
        i = line.find(field)
        if i >= 0:
            return line[:i] + ' }' + (',' if line.rstrip().endswith(',') else '')
    return line


def device(path):
    text = Path(path).read_text()
    by_pos = {k['pos']: k for k in DOM['keys']}
    lines = text.split('\n')
    out, in_controls = [], False
    for line in lines:
        if line.startswith('  "controls": ['):
            in_controls = True
        elif in_controls and line.startswith('  ],'):
            in_controls = False
        m = re.match(r'^(\s*\{ "id": .*"pos": (\d+),.*\})(,?)\s*$', line) if in_controls else None
        if m:
            line = cut_at_ours(line)
            m = re.match(r'^(\s*\{ "id": .*"pos": (\d+),.*?) \}(,?)\s*$', line)
            pos = int(m.group(2))
            key = by_pos.get(pos)
            extra = ''
            if key is not None and runs_of(key):
                if pos in CENTERD:
                    extra += ', "centered": true'
                extra += f', "print": {compact(runs_of(key))}'
            line = f'{m.group(1)}{extra} }}{m.group(3)}'
        out.append(line)
    text = '\n'.join(out)
    # the core's own print, after the art it describes (once)
    text = re.sub(r'  // How the drawing\'s legends are printed.*?\n  \},\n', '', text, flags=re.S)
    text = text.replace('  "art": "art/z10.svg",\n', '  "art": "art/z10.svg",\n\n' + OWN, 1)
    if HEADER_NEW not in text:
        assert HEADER_OLD in text, 'the controls\' header comment'
        text = text.replace(HEADER_OLD, HEADER_NEW, 1)
    tmp = Path(str(path) + '.tmp')
    tmp.write_text(text)
    tmp.replace(path)


def fonts():
    from fontTools import subset
    from fontTools.ttLib import TTFont
    FONTS.mkdir(exist_ok=True)
    barlow = FONTS / 'BarlowSemiCondensed-SemiBold.ttf'
    # (the art's typeface: the prototype's copy of Barlow Semi Condensed SemiBold, kept as it is)
    assert barlow.is_file(), barlow
    # DejaVu Sans Bold's signs alone, renamed as its license asks of a changed font
    f = TTFont(DEJAVU)
    opts = subset.Options()
    opts.name_IDs = ['*']
    opts.notdef_outline = True
    s = subset.Subsetter(opts)
    s.populate(text=SIGNS)
    s.subset(f)
    names = {1: 'Z10 Symbols', 2: 'Bold', 3: 'Z10 Symbols Bold', 4: 'Z10 Symbols Bold', 6: 'Z10Symbols-Bold', 16: 'Z10 Symbols', 17: 'Bold'}
    for rec in list(f['name'].names):
        if rec.nameID in names:
            rec.string = names[rec.nameID]
    f.save(FONTS / 'Z10Symbols-Bold.ttf')
    ofl = OFL.read_text().split('\n')
    body = '\n'.join(ofl[next(i for i, l in enumerate(ofl) if 'This Font Software is licensed' in l):])
    (FONTS / 'OFL.txt').write_text('Copyright 2017 The Barlow Project Authors (https://github.com/jpt/barlow)\n\n' + body)
    shutil.copy(DEJAVU_LICENSE, FONTS / 'DejaVu-LICENSE.txt')
    (FONTS / 'README.md').write_text(
        '# The Z10 core\'s typefaces\n\n'
        'The typefaces the Z10\'s art draws its legends in, for Ectomatter to set them anew (the Key lab\'s Legends section).\n\n'
        '- `BarlowSemiCondensed-SemiBold.ttf`: Barlow Semi Condensed SemiBold, by The Barlow Project Authors, under the SIL Open Font License 1.1 (`OFL.txt`).\n'
        '- `Z10Symbols-Bold.ttf`: the signs Barlow lacks (' + ' '.join(SIGNS) + '), cut from DejaVu Sans Bold 2.37 and renamed, as its license asks of a changed font (`DejaVu-LICENSE.txt`). Built by `tools/z10-art/print.py fonts`.\n')
    print('fonts written:', sorted(p.name for p in FONTS.iterdir()))


if __name__ == '__main__':
    if sys.argv[1:2] == ['fonts']:
        fonts()
    elif sys.argv[1:2] == ['device'] and len(sys.argv) == 3:
        device(sys.argv[2])
    else:
        sys.exit(__doc__)
