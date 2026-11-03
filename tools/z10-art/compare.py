"""Draw the Z10 both ways in WebKit and measure the difference.

  reference  the app's page as it draws the board: from the Z10 core's art
  candidate  art/z10.svg alone, lit the way the protocol says:
             an illumination takes the key's color and its light's strength,
             a lit legend takes the color mixed with its print gray

Both at 1240 CSS pixels across and zoom 2, on the app's ground, for each
lighting state. The live display is left out of the measure; it is content.
Writes out/<state>-ref.png, -svg.png and -diff.png, and prints the numbers.
Offscreen: no window opens.
"""
import json
import os
import sys
from pathlib import Path

os.environ.setdefault('GDK_BACKEND', 'x11')      # scale 1, as the main screen is
os.environ.setdefault('WEBKIT_DISABLE_DMABUF_RENDERER', '1')
os.environ.setdefault('WEBKIT_DISABLE_COMPOSITING_MODE', '1')
import gi  # noqa: E402
gi.require_version('Gtk', '3.0'); gi.require_version('WebKit2', '4.1')
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402
import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SVG = ROOT / 'art' / 'z10.svg'
OUTD = HERE / 'out'; OUTD.mkdir(exist_ok=True)
TIES = json.loads((HERE / 'ties.json').read_text())
DOM = json.loads((HERE / 'dom.json').read_text())
STATES = sys.argv[1:] or ['unlit', 'red', 'mix']
# the Z10 core's art, as the backend hands it to the page (core_art in src-tauri)
sys.path.insert(0, str(HERE))
from core import draft4, read  # noqa: E402
ART_JSON = json.dumps({'svg': SVG.read_text(), 'device': draft4(read(ROOT / 'device.json'), SVG.read_text())})
ZOOM = 1

# each state: pos -> [css color, strength]; the same numbers go to both sides
LIGHT = r"""
const STATE = @STATE@;
function lightOf(pos, cx) {
  if (STATE === 'unlit') return null;
  if (STATE === 'red') return ['rgb(255,45,0)', 1];
  const h = (cx / 649) * 360, a = 0.25 + 0.75 * ((pos * 37) % 11) / 10;
  return [`hsl(${h.toFixed(1)} 100% 50%)`, +a.toFixed(3)];
}
"""

REF_JS = LIGHT + r"""
document.body.replaceChildren(document.querySelector('#board'));
document.body.style.margin = '0';
const b = document.querySelector('#board'); b.style.maxWidth = 'none'; b.style.width = '1240px'; b.style.margin = '0';
for (const k of S.layout) {
  if (!k.led) continue;
  const v = lightOf(k.pos, k.cx); if (!v) continue;
  k.lc = v[0]; k.la = String(v[1]);
  k.el.style.setProperty('--c', v[0]); k.el.style.setProperty('--a', v[1]);
}
drawPanel();
'ok'
"""

CAND_JS = LIGHT + r"""
const ties = @TIES@;
const gray = [158, 158, 158];
const rgbOf = c => { const d = document.createElement('div'); d.style.color = c; document.body.append(d);
  const m = getComputedStyle(d).color.match(/[\d.]+/g).map(Number); d.remove(); return m; };
for (const t of ties.controls) {
  if (t.pos === undefined || !t.illumination) continue;
  const cx = +document.getElementById('control_' + t.control).getBBox().x + document.getElementById('control_' + t.control).getBBox().width / 2;
  const v = lightOf(t.pos, cx);
  const [c, a] = v || ['#fff', 0];
  const [under, over, glow] = t.illumination.map(n => document.getElementById('illumination_' + n));
  under.style.opacity = a; under.querySelectorAll('path').forEach(p => p.setAttribute('fill', c));
  over.style.opacity = 0.07 * a; over.querySelectorAll('path').forEach(p => p.setAttribute('fill', c));
  if (glow) { glow.style.opacity = a; glow.setAttribute('fill', c); glow.setAttribute('color', c); }
  const leg = document.getElementById('legend_' + t.legend);
  if (leg) {
    const rgb = rgbOf(c), m = rgb.map((v, i) => Math.round(v * a + gray[i] * (1 - a)));
    const mix = `rgb(${m.join(',')})`;
    leg.setAttribute('fill', mix); leg.setAttribute('color', mix);
  }
}
'ok'
"""


def render(uri_or_html, js, out, is_html=False):
    w = Gtk.OffscreenWindow()
    ucm = WebKit2.UserContentManager()
    # no keyboard: every call to the app's backend fails, so the page draws the board from its own files only
    ucm.add_script(WebKit2.UserScript(
        "window.__ART = " + ART_JSON + ";"
        "window.fetch = (f => (u, o) => String(u).includes('/api/core-art') ? Promise.resolve(new Response(JSON.stringify(window.__ART), { headers: { 'content-type': 'application/json' } }))"
        " : String(u).includes('/api/') ? Promise.reject(new Error('no keyboard')) : f(u, o))(window.fetch.bind(window));",
        WebKit2.UserContentInjectedFrames.TOP_FRAME, WebKit2.UserScriptInjectionTime.START, None, None))
    v = WebKit2.WebView.new_with_user_content_manager(ucm)
    v.set_zoom_level(ZOOM)
    w.add(v); w.set_default_size(1240 * ZOOM, 520 * ZOOM); w.show_all()
    done = {}

    def shot():
        def fin(view, res):
            view.get_snapshot_finish(res).write_to_png(str(out))
            Gtk.main_quit()
        v.get_snapshot(WebKit2.SnapshotRegion.FULL_DOCUMENT, WebKit2.SnapshotOptions.NONE, None, fin)
        return False

    def ran(view, res, *_):
        try:
            view.evaluate_javascript_finish(res)
        except Exception as e:
            print('script failed:', e); Gtk.main_quit(); return
        GLib.timeout_add(1500, shot)

    def loaded(view, ev):
        if ev == WebKit2.LoadEvent.FINISHED and not done:
            done['y'] = 1
            GLib.timeout_add(2500, lambda: v.evaluate_javascript(js, -1, None, None, None, ran, None) and False)

    v.connect('load-changed', loaded)
    if is_html:
        v.load_html(uri_or_html, 'file:///')
    else:
        v.load_uri(uri_or_html)
    GLib.timeout_add_seconds(60, lambda: (print('TIMEOUT'), Gtk.main_quit()))
    Gtk.main()


def load(path):
    return np.asarray(Image.open(path).convert('RGB')).astype(np.int16)


svg_text = SVG.read_text()
page = ('<!doctype html><html><head><style>html,body{margin:0;background:#202020}svg{display:block;width:1240px;height:auto}</style>'
        '</head><body>' + svg_text + '</body></html>')
px, py, pw, ph = DOM['panel']
for st in STATES:
    ref, cand, diff = OUTD / f'{st}-ref.png', OUTD / f'{st}-svg.png', OUTD / f'{st}-diff.png'
    render('http://localhost:5173/?livecaps' if 'live' in os.environ.get('Z10_REF', '') else 'http://localhost:5173/',
           (('window.__NOTOP=1;' if os.environ.get('NOTOP') else '') + REF_JS.replace('@STATE@', json.dumps(st))), ref)
    render(page, CAND_JS.replace('@STATE@', json.dumps(st)).replace('@TIES@', json.dumps(TIES)), cand, is_html=True)
    a, b = load(ref), load(cand)
    sx = a.shape[1] / 649                       # image pixels per drawing unit
    H = round(260 * sx)
    a, b = a[:H], b[:H]
    panel = (int((px - 6) * sx) - 2, int((py + 2) * sx) - 2, int((px - 6 + pw) * sx) + 3, int((py + 2 + ph) * sx) + 3)
    if a.shape != b.shape:
        print(st, 'sizes differ', a.shape, b.shape); continue
    d = np.abs(a - b).max(axis=2)
    x0, y0, x1, y1 = panel
    d[y0:y1, x0:x1] = 0
    Image.fromarray(np.clip(d * 8, 0, 255).astype(np.uint8)).save(diff)
    print(f'{st:6s} mean {d.mean():.3f}  p99 {np.percentile(d, 99):.0f}  p99.9 {np.percentile(d, 99.9):.0f}  '
          f'max {d.max()}  over 8: {(d > 8).mean() * 100:.2f}%  over 24: {(d > 24).mean() * 100:.3f}%')
