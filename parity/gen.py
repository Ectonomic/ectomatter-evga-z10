"""Parity vectors: what the original Python editor does, for the Rust core to match.

The Rust core replaces the Python keyboard code in ~/dev/z10-rgb. This runs
that code on thousands of inputs and writes what it produced, byte for byte:
decoded profiles and every settings edit, preview frames, macros both ways,
display frames, and every exchange with the keyboard for each command and
each editor action. The Rust tests replay the same inputs and must produce
exactly the same output.

    python3 parity/gen.py [path to z10-rgb] [backup dir ...]

Synthetic cases go to parity/vectors.json.gz, which is committed. Cases built
from a real keyboard's backups go to parity/private/, which is not.
"""
import gzip
import json
import os
import random
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
Z10RGB = Path(sys.argv[1] if len(sys.argv) > 1 else Path.home() / 'dev/z10-rgb')
sys.path.insert(0, str(Z10RGB))
sys.path.insert(0, str(Path.home() / 'dev/z10-lcd'))

from z10rgb import device as D          # noqa: E402

rng = random.Random(0x2611)
EDGES = [0, 1, 2, 3, 4, 7, 8, 15, 16, 127, 128, 254, 255]


def rbyte():
    """A byte, often one of the values the code treats specially."""
    return rng.choice(EDGES) if rng.random() < 0.5 else rng.randrange(256)
hx = bytes.hex


def fix_profile_checksum(raw):
    raw = bytearray(raw)
    raw[7] = D.csum(raw[8:])
    return bytes(raw)


def synthetic_profile():
    """A profile of random bytes, with plausible header and name."""
    raw = bytearray(rng.randbytes(D.SIZE[8]))
    raw[0:7] = bytes([8, 0xEA, 0x02, 1, rng.choice([1, 2, 3, 4, 5, 254]), 0, D.SUCCESS])
    if rng.random() < 0.7:          # most profiles hold a real name
        p = D.Profile(raw)
        p.name = rng.choice(['DEFAULT 1', 'Gaming', 'Écrire', '日本語', 'x' * 70, '', 'Emoji 🎮 here'])
        raw = p.raw
    if rng.random() < 0.8:
        raw[D.Profile.O_MODE] = rng.randrange(8)
    return fix_profile_checksum(raw)


# ---------------------------------------------------------------- lighting edits

def rand_argb():
    return dict(brightness=rng.randrange(256), color=[rng.randrange(256) for _ in range(3)])


def random_patch(view):
    """A random subset of the lighting view, with fresh values."""
    patch = {}
    if rng.random() < 0.7:
        patch['mode'] = rng.choice(D.MODES)
    if rng.random() < 0.2:
        patch['sub'] = rng.randrange(256)
    for key in ('static', 'breathing', 'pulse', 'spiral', 'wave', 'trigger', 'stars'):
        if rng.random() < 0.4:
            continue
        e = {}
        for k, v in view[key].items():
            if rng.random() < 0.5:
                continue
            if k in ('cycle', 'shift'):
                e[k] = rng.randrange(65536)
            elif k == 'colors':
                n = rng.choice([0, 1, 2, 3, 7, 8, 9]) if key != 'breathing' else rng.choice([0, 1, 2])
                cols = [rand_argb() for _ in range(n)]
                if key == 'breathing':
                    for c in cols:
                        if rng.random() < 0.7:
                            c['hold'] = rng.randrange(256)
                e[k] = cols
            elif k == 'background':
                e[k] = rand_argb()
            elif k == 'color':
                e['brightness'] = rng.randrange(256)
                e['color'] = [rng.randrange(256) for _ in range(3)]
            elif k == 'brightness' and key == 'static':
                continue
            else:
                e[k] = rbyte()
        patch[key] = e
    if rng.random() < 0.3:
        patch['lcd'] = {k: v for k, v in (('color', rand_argb()), ('apps', rng.randrange(256))) if rng.random() < 0.7}
    if rng.random() < 0.2:
        patch['game_keys'] = rng.randrange(256)
    return patch


BAD_PATCHES = [
    {'mode': 'rainbow'}, {'mode': 3}, {'sub': 256}, {'sub': -1},
    {'static': {'zone': 300}}, {'static': {'color': [1, 2, 3]}},
    {'breathing': {'cycle': 70000}}, {'pulse': {'count': 999}},
    {'wave': {'colors': [{'color': [1, 2, 3]}]}}, {'lcd': {'apps': 256}},
]


def profile_cases(raws, n_edits):
    out = []
    for raw in raws:
        p = D.Profile.decode(raw)
        view = p.lighting()
        case = dict(raw=hx(raw), name=p.name, lighting=view, keys=p.keys(), fnkeys=p.keys(True),
                    checksum=p.checksum(), encode={str(n): hx(p.encode(n)) for n in (1, 3, 5)},
                    edits=[], names=[])
        for _ in range(n_edits):
            patch = random_patch(view)
            q = D.Profile(raw)
            q.set_lighting(patch)
            case['edits'].append(dict(patch=patch, raw=hx(bytes(q.raw)), lighting=q.lighting()))
        for patch in BAD_PATCHES:
            q = D.Profile(raw)
            try:
                q.set_lighting(patch)
                case['edits'].append(dict(patch=patch, raw=hx(bytes(q.raw))))
            except Exception:
                case['edits'].append(dict(patch=patch, error=True))
        for s in ['Work', 'x' * 80, 'Ünïcødé', '🎮🎹🎧', 'a🎮' * 40, '', 'pad\0\0', '\0lead', ' spaced ']:
            q = D.Profile(raw)
            q.name = s
            case['names'].append(dict(name=s, raw=hx(bytes(q.raw)), read=q.name))
        keys = []
        for _ in range(3):
            q = D.Profile(raw)
            pos = rng.randrange(D.NKEYS)
            define = [rng.randrange(256) for _ in range(4)]
            sec = rng.random() < 0.5
            q.set_key(pos, define, sec)
            keys.append(dict(pos=pos, define=define, secondary=sec, raw=hx(bytes(q.raw))))
        case['set_keys'] = keys
        out.append(case)
    return out


# ---------------------------------------------------------------- preview frames

def preview_cases(raws):
    out = []
    for raw in raws:
        L = D.Profile.decode(raw).lighting()
        for mode in D.MODES:
            e = L[mode] if mode != 'off' else {}
            for t in (0, 40, 999, 1000, 123456):
                for restart in (False, True):
                    out.append(dict(mode=mode, e=e, t=t, restart=restart, frame=hx(D.preview_report(mode, e, t, restart))))
    # the editor's own drafts can leave fields out
    for mode, key, top in (('spiral', 'direction', 1), ('wave', 'direction', 3), ('trigger', 'submode', 2)):
        for v in (top - 1, top, top + 1, 255):
            out.append(dict(mode=mode, e={key: v}, t=5, restart=False, frame=hx(D.preview_report(mode, {key: v}, 5, False))))
    for mode, e in [('static', {}), ('static', {'color': None}), ('breathing', {}), ('pulse', {'background': None}),
                    ('pulse', {'cycle': 0, 'colors': [rand_argb()]}), ('trigger', {'shift': 500, 'key': 7}),
                    ('trigger', {'cycle': None, 'shift': 500}), ('spiral', {'direction': 255}), ('wave', {'direction': 2}),
                    ('wave', {'direction': 7}), ('stars', {'cycle': 3000, 'brightness': 9})]:
        out.append(dict(mode=mode, e=e, t=777, restart=False, frame=hx(D.preview_report(mode, e, 777, False))))
    return out


# ---------------------------------------------------------------- macros

def random_steps(n):
    steps = []
    if rng.random() < 0.1:
        steps.append(dict(wait=rng.randrange(3000), delay=0))
    for _ in range(n):
        r = rng.random()
        delay = rng.choice([0, 0, 10, 250, 65535, 70000])
        if r < 0.15:
            steps.append(dict(media=rng.choice(list(D.MACRO_MEDIA)), down=rng.random() < 0.5, delay=delay))
        else:
            steps.append(dict(code=rng.randrange(256), down=rng.random() < 0.5, delay=delay))
    return steps


def macro_raw(index, name, steps, run=9, repeat=0, unit=1):
    buf = bytearray(D.SIZE[9])
    buf[0:7] = bytes([9, 0xEA, 0x02, 1, index, 0, D.SUCCESS])
    body = D.macro_body(name, steps, run, repeat, unit)
    buf[7:7 + len(body)] = body
    return bytes(buf)


def macro_cases(raws):
    out = dict(reports=[], encode=[], bodies=[])
    reports = list(raws)
    for i in range(40):
        reports.append(macro_raw(rng.randrange(1, 101), rng.choice(['Copy', 'Paste', 'KDE Switch Desktop Left', 'Ünï', 'x' * 60]),
                                 random_steps(rng.randrange(0, 300))))
    for _ in range(40):           # arbitrary record streams, including ones the editor would not write
        raw = bytearray(rng.randbytes(D.SIZE[9]))
        raw[0:7] = bytes([9, 0xEA, 0x02, 1, 5, 0, D.SUCCESS])
        raw[62:64] = rng.randrange(0, 1100).to_bytes(2, 'little')
        reports.append(bytes(raw))
    for raw in reports:
        steps = D.macro_steps(raw)
        n = int.from_bytes(raw[62:64], 'little')
        out['reports'].append(dict(raw=hx(raw), steps=steps,
                                   meta=dict(name=bytes(raw[9:9 + min(raw[8], 50)]).decode('ascii', 'replace'), method=raw[59],
                                             repeat=raw[60], unit=raw[61], length=n),
                                   editable=D.macro_data(steps) == bytes(raw[64:64 + n])))
    for _ in range(120):
        steps = random_steps(rng.randrange(0, 400))
        out['encode'].append(dict(steps=steps, data=hx(D.macro_data(steps))))
    for _ in range(60):
        name = rng.choice(['Macro', 'Ctrl+Alt+Del', 'Ünïcødé 🎮', 'y' * 70, ''])
        steps = random_steps(rng.randrange(0, 50))
        run, repeat, unit = rng.randrange(256), rng.randrange(256), rng.randrange(256)
        out['bodies'].append(dict(name=name, steps=steps, run=run, repeat=repeat, unit=unit,
                                  body=hx(D.macro_body(name, steps, run, repeat, unit))))
    return out


# ---------------------------------------------------------------- recording

def recorder_cases():
    from z10rgb import recorder as R
    import threading
    tables = dict(usage={str(k): v for k, v in R.KEY_TO_USAGE.items()}, media={str(k): v for k, v in R.KEY_TO_MEDIA.items()})
    codes = list(R.KEY_TO_USAGE) + list(R.KEY_TO_MEDIA) + [0, 240, 248, 330, 272]
    out = []
    for _ in range(60):
        room, waits = rng.choice([967, 40, 12, 0]), rng.random() < 0.7
        rec = R.Recorder()
        st = dict(fds=[], steps=[], last=None, t0=0, seen=0, room=room, used=0, waits=waits, skipped=0,
                  full=False, done=False, reason='', held=set())
        t, events = 1000.0, []
        for _ in range(rng.randrange(0, 60)):
            t += rng.choice([0, 0.0005, 0.001, 0.0125, 0.25, 1.5, 70.0])
            t = round(t, 6)
            events.append([t, rng.choice(codes), rng.random() < 0.55])
        for ev in events:
            rec._event(st, *ev)
        closing = R.Recorder._close_holds(st['steps'], st['held'])
        out.append(dict(room=room, waits=waits, events=events, steps=closing[:len(st['steps'])],
                        closing=sorted(closing[len(st['steps']):], key=lambda d: json.dumps(d, sort_keys=True)),
                        used=st['used'], full=st['full'], skipped=st['skipped'], done=st['done']))
    return dict(tables=tables, cases=out)


# ---------------------------------------------------------------- display

def lcd_cases():
    import z10lcd
    out = []
    for rows in [[], ['CPU 62°  4% 3831MHz', 'GPU 41°', 'RAM 14.3 / 60.2 GB', 'VRAM 3.3 / 31.8 GB'],
                 ['€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“', '”•–—˜™š›œžŸ', '日本語', 'x' * 30],
                 ['tab\there', 'extra', 'rows', 'here', 'and more'],
                 ['\u0081\u008d ÿ', '🎮']]:
        out.append(dict(rows=rows, frame=hx(z10lcd.frame(rows))))
    return out


# ---------------------------------------------------------------- the keyboard itself

class Sim:
    """A pretend Z10 good enough for the editor's commands: it keeps profiles
    and macro slots and answers each request the way the keyboard does. The
    log of every report sent and every answer read is what the Rust core is
    checked against."""

    def __init__(self, profiles, macros, active=1, fw=0x01AB):
        self.profiles = {n: bytearray(r) for n, r in profiles.items()}
        self.macros = {i: bytearray(r) for i, r in macros.items()}
        self.active, self.fw = active, fw
        self.last = None
        self.log = []

    def answer(self, req):
        rid = req[0]
        r = bytearray(D.SIZE[rid])
        r[0:6] = req[0:6]
        r[6] = D.SUCCESS
        if rid == 4:
            main, sub1 = req[3], req[4]
            if main == D.READ_FW:
                r[7:9] = self.fw.to_bytes(2, 'little')
            elif main == D.PROFILE_NUM:
                if sub1 == 1:
                    r[7] = self.active
                else:
                    self.active = req[7]
        elif rid == 8:
            n = req[4]
            if req[3] == 1:
                if n not in self.profiles:
                    r[6] = D.FAIL
                else:
                    r[:] = self.profiles[n]
                    r[0:7] = bytes([8, 0xEA, 0x02, 1, n, 0, D.SUCCESS])
            else:
                self.profiles[n] = bytearray(req)
        elif rid == 9:
            i = req[4]
            if req[3] == 1:
                if i == 0:
                    for j in range(1, 101):
                        r[7 + j] = 1 if j in self.macros else 0
                elif i in self.macros:
                    r[:] = self.macros[i]
                    r[0:7] = bytes([9, 0xEA, 0x02, 1, i, 0, D.SUCCESS])
                else:
                    r[8] = 0xFF
            else:
                if req[8] == 0xFF:
                    self.macros.pop(i, None)
                else:
                    self.macros[i] = bytearray(req)
        return bytes(r)

    # the two calls z10rgb.device.Z10 makes
    def set(self, buf):
        buf = bytes(buf)
        assert len(buf) == D.SIZE[buf[0]]
        self.log.append(['set', hx(buf)])
        self.last = self.answer(buf)

    def get(self, rid):
        r = self.last if self.last and self.last[0] == rid else bytes(D.SIZE[rid])
        self.log.append(['get', hx(r)])
        return r


def fake_z10(sim):
    z = D.Z10.__new__(D.Z10)
    z.path, z.fd = 'fake', -1
    z.set, z.get = sim.set, sim.get
    z.close = lambda: None
    return z


def device_cases(profiles, macros):
    """Each command on its own, then each editor action, with the full log."""
    out = []
    import time
    real_sleep = time.sleep
    D.time.sleep = lambda s: None

    def run(name, fn, **args):
        sim = Sim(profiles, macros)
        z = fake_z10(sim)
        try:
            result = json.loads(json.dumps(fn(z)))
            err = False
        except Exception:
            result, err = None, True
        out.append(dict(name=name, log=sim.log, result=result, error=err, args=args))

    run('firmware_version', lambda z: list(z.firmware_version()))
    run('profile_number', lambda z: z.profile_number())
    for n in (1, 5):
        run(f'select_profile {n}', lambda z, n=n: z.select_profile(n))
    run('reload_profile', lambda z: z.reload_profile())
    for n in (1, 2, 254, 7):
        run(f'read_profile {n}', lambda z, n=n: hx(bytes(z.read_profile(n).raw)))
    run('write_profile 3', lambda z: z.write_profile(3, D.Profile.decode(profiles[1])), n=3, raw=hx(profiles[1]))
    run('read_macros', lambda z: {str(k): v for k, v in z.read_macros().items()})
    body = D.macro_body('Test', random_steps(20))
    run('write_macro 42', lambda z: z.write_macro(42, body), index=42, body=hx(body))
    run('delete_macro 2', lambda z: z.delete_macro(2))
    colors = [[rng.randrange(256) for _ in range(3)] for _ in range(116)]
    run('stream_colors', lambda z: z.stream_colors(colors, 200), colors=colors, brightness=200)
    run('stop_stream', lambda z: z.stop_stream())
    run('exit_sync', lambda z: z.exit_sync())
    D.time.sleep = real_sleep
    return out


def editor_cases(profiles, macros):
    """The editor's actions through z10rgb.server.Keyboard, backups off."""
    with tempfile.TemporaryDirectory() as tmp:
        os.environ['XDG_DATA_HOME'] = tmp
        from z10rgb import server as S
        D.time.sleep = lambda s: None
        out = []

        def run(name, fn, **extra):
            sim = Sim(profiles, macros)
            S.Z10 = lambda: fake_z10(sim)
            kb = S.Keyboard.__new__(S.Keyboard)
            import threading
            kb.lock, kb.backed_up = threading.Lock(), True

            class NoLive:
                owner = None
                def status(self):
                    return dict(mode=None, error=None)
            kb.live = NoLive()
            try:
                result, err = json.loads(json.dumps(fn(kb))), False
            except Exception:
                result, err = None, True
            out.append(dict(name=name, log=sim.log, result=result, error=err, **extra))

        run('state', lambda kb: kb.state())
        view = D.Profile.decode(profiles[2]).lighting()
        for i in range(12):
            body = dict(lighting=random_patch(view))
            if i % 3 == 0:
                body['name'] = rng.choice(['Work', 'Game night', 'z' * 70])
            if i % 4 == 0:
                keys = D.Profile.decode(profiles[2]).keys()
                keys[rng.randrange(116)] = [rng.randrange(256) for _ in range(4)]
                body['keys'] = keys
            if i % 5 == 0:
                body['fnkeys'] = D.Profile.decode(profiles[3]).keys(True)
            n = rng.choice([1, 2, 5])
            run(f'save {n}', lambda kb, n=n, body=body: kb.save(n, body), n=n, body=body)
        for i, body in enumerate([dict(name='New Macro 1', steps=random_steps(12)),
                                  dict(name='Plays twice', steps=random_steps(5), run=11, repeat=2, unit=1),
                                  dict(delete=True), dict(steps=[])]):
            idx = 7 if i < 2 else 2
            run(f'write_macro {idx}', lambda kb, idx=idx, body=body: kb.write_macro(idx, body), index=idx, body=body)
        run('activate 4', lambda kb: kb.activate(4), n=4)
        return out


def main():
    backups = [Path(a) for a in sys.argv[2:]]
    # synthetic ---------------------------------------------------------------
    profs = [synthetic_profile() for _ in range(16)]
    sim_profiles = {n: profs[i] for i, n in enumerate((1, 2, 3, 4, 5, 254))}
    sim_macros = {i: macro_raw(i, f'Macro {i}', random_steps(rng.randrange(1, 60))) for i in (2, 3, 9, 50, 100)}
    vectors = dict(
        profiles=profile_cases(profs, 20),
        preview=preview_cases(profs[:6]),
        macros=macro_cases([]),
        lcd=lcd_cases(),
        recorder=recorder_cases(),
        device=dict(profiles={str(k): hx(v) for k, v in sim_profiles.items()}, macros={str(k): hx(v) for k, v in sim_macros.items()},
                    commands=device_cases(sim_profiles, sim_macros), editor=editor_cases(sim_profiles, sim_macros)),
    )
    f = HERE / 'vectors.json.gz'
    f.write_bytes(gzip.compress(json.dumps(vectors, ensure_ascii=False, separators=(',', ':')).encode(), mtime=0))
    print('synthetic vectors:', f.stat().st_size // 1024, 'KB')

    # from real backups, kept private -------------------------------------------
    for b in backups:
        profiles = {int(f.stem.split('-')[1]): f.read_bytes() for f in sorted(b.glob('profile-*.bin'))}
        macros = {int(f.stem.split('-')[1]): f.read_bytes() for f in sorted(b.glob('macro-*.bin'))}
        real = dict(
            profiles=profile_cases(list(profiles.values()), 60),
            preview=preview_cases(list(profiles.values())),
            macros=macro_cases(list(macros.values())),
            lcd=[],
            device=dict(profiles={str(k): hx(v) for k, v in profiles.items()}, macros={str(k): hx(v) for k, v in macros.items()},
                        commands=device_cases(profiles, macros), editor=editor_cases(profiles, macros)),
        )
        d = HERE / 'private'
        d.mkdir(exist_ok=True)
        f = d / f'{b.name}.json.gz'
        f.write_bytes(gzip.compress(json.dumps(real, ensure_ascii=False, separators=(',', ':')).encode(), mtime=0))
        print('private vectors:', f.name, f.stat().st_size // 1024, 'KB')


if __name__ == '__main__':
    main()
