"""Read the Z10 board out of the running page, as WebKit lays it out.

One-time migration step: writes dom.json with every part of the board in the
drawing's own units (the space of board.svg's viewBox). Needs the dev server
on :5173. Runs in an offscreen WebKit view; no window opens.
"""
import json, os, sys
os.environ.setdefault('WEBKIT_DISABLE_DMABUF_RENDERER', '1')
os.environ.setdefault('WEBKIT_DISABLE_COMPOSITING_MODE', '1')
import gi
gi.require_version('Gtk', '3.0'); gi.require_version('WebKit2', '4.1')
from gi.repository import GLib, Gtk, WebKit2

OUT = os.path.join(os.path.dirname(__file__), 'dom.json')
URL = 'http://localhost:5173/?livecaps'
STUB = "window.fetch = (f => (u, o) => String(u).includes('/api/') ? Promise.reject(new Error('no keyboard')) : f(u, o))(window.fetch.bind(window));"
JS = r"""
(() => {
  const board = document.querySelector('#board');
  const B = board.getBoundingClientRect();
  const [vx, vy, vw, vh] = [6, -2, 649, 260];
  const s = vw / B.width;
  const X = x => vx + (x - B.left) * s, Y = y => vy + (y - B.top) * s;
  const box = r => [X(r.left), Y(r.top), r.width * s, r.height * s];
  const cs = e => getComputedStyle(e);
  const opacityTo = (e, stop) => { let o = 1; for (let n = e; n && n !== stop.parentElement; n = n.parentElement) o *= +cs(n).opacity; return o; };
  const runs = (root, stop) => {
    const out = [];
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let t; (t = w.nextNode());) {
      if (!t.data.trim()) continue;
      const p = t.parentElement, st = cs(p);
      if (st.display === 'none' || st.visibility === 'hidden') continue;
      const chars = [];
      let i = 0;
      for (const ch of t.data) {
        const r = document.createRange(); r.setStart(t, i); r.setEnd(t, i + ch.length); i += ch.length;
        const rr = r.getClientRects()[0]; if (!rr) continue;
        chars.push({ ch, box: box(rr) });
      }
      const mk = document.createElement('span'); mk.style.cssText = 'display:inline-block;width:0;height:0;vertical-align:baseline';
      p.appendChild(mk); const base = Y(mk.getBoundingClientRect().bottom); mk.remove();
      const m = new DOMMatrix(st.transform === 'none' ? undefined : st.transform);
      out.push({ text: t.data, cls: p.className, font: st.fontFamily, weight: st.fontWeight, size: parseFloat(st.fontSize) * s,
                 spacing: parseFloat(st.letterSpacing) * s || 0, color: st.color, opacity: opacityTo(p, stop), scale: m.a, base, chars });
    }
    return out;
  };
  const keys = [...board.querySelectorAll('.key')].map(k => {
    const icons = [...k.querySelectorAll('.icon')].map(i => ({ cls: i.className, box: box(i.getBoundingClientRect()), mask: cs(i).webkitMaskImage || cs(i).maskImage, color: cs(i).backgroundColor, opacity: opacityTo(i, k) }));
    const imgs = [...k.querySelectorAll('img')].map(i => ({ src: i.getAttribute('src'), box: box(i.getBoundingClientRect()), opacity: opacityTo(i, k) }));
    return { pos: +k.dataset.pos, label: k.getAttribute('aria-label'), cls: k.className, box: box(k.getBoundingClientRect()),
             leg: runs(k.querySelector('.leg'), k), icons, imgs };
  });
  const prints = [...board.querySelectorAll('.print')].map(p => ({ box: box(p.getBoundingClientRect()), runs: runs(p, p) }));
  const knobs = [...board.querySelectorAll('.knob')].map(k => box(k.getBoundingClientRect()));
  const screen = box(board.querySelector('.screen').getBoundingClientRect());
  const panel = box(board.querySelector('.screen .panel').getBoundingClientRect());
  const glass = box(board.querySelector('.glass').getBoundingClientRect());
  return JSON.stringify({ board: [B.width, B.height], scale: s, cqw: B.width / 100 * s, keys, prints, knobs, screen, panel, glass });
})()
"""
w = Gtk.OffscreenWindow()
ucm = WebKit2.UserContentManager()
ucm.add_script(WebKit2.UserScript(STUB, WebKit2.UserContentInjectedFrames.TOP_FRAME, WebKit2.UserScriptInjectionTime.START, None, None))
v = WebKit2.WebView.new_with_user_content_manager(ucm)
w.add(v); w.set_default_size(1400, 900); w.show_all()

def got(view, res, *_):
    try:
        val = view.evaluate_javascript_finish(res).to_string()
        json.dump(json.loads(val), open(OUT, 'w'), indent=1)
        d = json.loads(val); print('wrote', OUT, 'keys', len(d['keys']), 'board', d['board'])
    except Exception as e:
        print('ERR', e)
    Gtk.main_quit()

def go():
    v.evaluate_javascript("document.body.replaceChildren(document.querySelector('#board')); document.body.style.margin='0'; const b=document.querySelector('#board'); b.style.maxWidth='none'; b.style.width='1240px'; b.style.margin='0'", -1, None, None, None, None, None)
    GLib.timeout_add(1500, lambda: v.evaluate_javascript(JS, -1, None, None, None, got, None) and False)
    return False
v.connect('load-changed', lambda view, ev: ev == WebKit2.LoadEvent.FINISHED and GLib.timeout_add(3000, go))
v.load_uri(URL)
GLib.timeout_add_seconds(40, lambda: (print('TIMEOUT'), Gtk.main_quit()))
Gtk.main()
