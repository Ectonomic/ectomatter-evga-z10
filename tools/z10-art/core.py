"""The Z10 core's device description as these tools read it.

The core's file is JSON with comments (the core protocol's draft 6): each
control names its piece of the drawing by number ("art"), and its legend and
lights follow from that number by rule. read() gives the description;
draft4() adds each control's pieces named as draft 4 had them, which the
app's page draws from.
"""
import json
from pathlib import Path


def uncommented(text):
    """JSON with its comments left out: // to the end of a line, and /* */, outside its strings."""
    out, i, string = [], 0, False
    while i < len(text):
        c = text[i]
        if string:
            out.append(c)
            if c == '\\':
                out.append(text[i + 1:i + 2])
                i += 1
            elif c == '"':
                string = False
        elif c == '"':
            string = True
            out.append(c)
        elif text.startswith('//', i):
            while i < len(text) and text[i] != '\n':
                i += 1
            continue
        elif text.startswith('/*', i):
            i = text.find('*/', i + 2)
            i = len(text) if i < 0 else i + 2
            continue
        else:
            out.append(c)
        i += 1
    return ''.join(out)


def read(path):
    return json.loads(uncommented(Path(path).read_text()))


def draft4(device, svg):
    """Each control's "control", "legend" (where the drawing has one) and, for a key with an LED,
    "illumination", unless it names them itself; each screen's "screen" and "overlay"."""
    for c in device['controls']:
        n = c['art']
        c['control'] = n
        if 'legend' not in c and f'id="legend_{100 + n}"' in svg:
            c['legend'] = 100 + n
        if 'illumination' not in c and 'slider' not in c and c.get('lit', True):
            c['illumination'] = [n, 200 + n, 400 + n]
    for s in device['screens']:
        s['screen'] = s['overlay'] = s['art']
    return device
