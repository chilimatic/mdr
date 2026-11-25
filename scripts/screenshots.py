"""Makes the README screenshots: runs mdr in a pseudo terminal, sends keys, and draws
the screen into a PNG.

    cargo build --release
    python3 -m venv /tmp/venv && /tmp/venv/bin/pip install pyte pillow
    /tmp/venv/bin/python scripts/screenshots.py target/release/mdr . docs/screenshots [NAME...]

Needs the DejaVu fonts. Images in documents are drawn with block characters here.
"""
import os, pty, sys, time, select, struct, fcntl, termios, re
import pyte
from PIL import Image, ImageDraw, ImageFont

FONT = '/usr/share/fonts/TTF/DejaVuSansMono{}.ttf'
SIZE = 15
BG, FG = (30, 33, 39), (220, 223, 228)
NAMED = {
    'black': (40, 44, 52), 'red': (224, 108, 117), 'green': (152, 195, 121), 'brown': (229, 192, 123),
    'yellow': (229, 192, 123), 'blue': (97, 175, 239), 'magenta': (198, 120, 221), 'cyan': (86, 182, 194),
    'white': (220, 223, 228),
}
FALLBACK = set('ℹ❗⛔')
fallback = None
APC = re.compile(rb'\x1b_[^\x1b]*\x1b\\')

def color(c, default):
    if c == 'default':
        return default
    if c in NAMED:
        return NAMED[c]
    if c.startswith('bright') and c[6:] in NAMED:
        return NAMED[c[6:]]
    try:
        return tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))
    except ValueError:
        return default

def run(binary, args, cols, rows, keys, cwd):
    scr = pyte.Screen(cols, rows)
    st = pyte.ByteStream(scr)
    env = {k: v for k, v in os.environ.items() if k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'COLORFGBG')}
    env.update(MDR_CONFIG=os.path.join(cwd, 'tests/fixtures/empty-config.toml'), XDG_STATE_HOME='/nonexistent-mdr-state', TERM='xterm-256color', COLORTERM='truecolor')
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.execve(binary, ['mdr'] + args, env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
    buf = bytearray()
    def pump(t):
        end = time.time() + t
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.05)
            if r:
                try:
                    buf.extend(os.read(fd, 1 << 16))
                except OSError:
                    return
    pump(3.0)
    os.write(fd, b'\x1b'); pump(0.3)  # the first key can be eaten by the terminal query
    for k, wait in keys:
        os.write(fd, k); pump(wait)
    pump(0.5)
    st.feed(APC.sub(b'', bytes(buf)))
    os.write(fd, b'q'); pump(0.3)
    try:
        os.kill(pid, 9); os.waitpid(pid, 0)
    except OSError:
        pass
    return scr

def render(scr, out):
    fonts = {(b, i): ImageFont.truetype(FONT.format({(0, 0): '', (1, 0): '-Bold', (0, 1): '-Oblique', (1, 1): '-BoldOblique'}[(b, i)]), SIZE) for b in (0, 1) for i in (0, 1)}
    cw = round(fonts[(0, 0)].getlength('M'))
    global fallback
    fallback = ImageFont.truetype('/usr/share/fonts/TTF/DejaVuSans-Bold.ttf', SIZE)
    asc, desc = fonts[(0, 0)].getmetrics()
    ch = asc + desc
    pad = 18
    img = Image.new('RGB', (scr.columns * cw + 2 * pad, scr.lines * ch + 2 * pad + 28), (33, 37, 43))
    d = ImageDraw.Draw(img)
    # Window bar with three dots.
    for k, c in enumerate([(224, 108, 117), (229, 192, 123), (152, 195, 121)]):
        d.ellipse((pad + k * 20, 9, pad + k * 20 + 11, 20), fill=c)
    top = 28 + pad
    d.rectangle((pad - 6, top - 6, pad + scr.columns * cw + 6, top + scr.lines * ch + 6), fill=BG)
    for y in range(scr.lines):
        line = scr.buffer[y]
        for x in range(scr.columns):
            c = line[x]
            fg, bg = color(c.fg, FG), color(c.bg, BG)
            if c.reverse:
                fg, bg = bg, fg
            px, py = pad + x * cw, top + y * ch
            if bg != BG:
                w = cw * (2 if c.data and len(c.data) == 1 and ord(c.data) > 0x1100 and x + 1 < scr.columns and line[x + 1].data == '' else 1)
                d.rectangle((px, py, px + w - 1, py + ch - 1), fill=bg)
            if c.data and c.data != ' ':
                f = fonts[(int(c.bold), int(c.italics))]
                if c.data in FALLBACK:
                    f = fallback
                # Block and box characters fill the cell; draw them a bit taller to close the gaps.
                d.text((px, py), c.data, font=f, fill=fg)
            if c.underscore:
                d.line((px, py + ch - 2, px + cw - 1, py + ch - 2), fill=fg)
            if c.strikethrough:
                d.line((px, py + ch // 2, px + cw - 1, py + ch // 2), fill=fg)
    img.save(out)

if __name__ == '__main__':
    binary, repo, outdir = sys.argv[1:4]
    J = lambda n: [(b'j', 0.05)] * n
    shots = {
        'document': (['docs/demo.md'], 100, 46, []),
        'diagrams': (['tests/fixtures/diagrams.md'], 100, 40, []),
        'class-diagram': (['tests/fixtures/diagrams.md'], 100, 44, [(b'/', 0.2)] + [(bytes([c]), 0.05) for c in b'Class diagram'] + [(b'\r', 0.4), (b'\x1b', 0.3)] + J(13)),
        'folder': (['.'], 120, 32, J(1)),
        'folder-search': (['.'], 120, 32, [(b's', 0.2)] + [(bytes([c]), 0.05) for c in b'mermaid'] + [(b'\r', 0.8), (b'j', 0.3)]),
    }
    only = sys.argv[4:]
    for name, (args, cols, rows, keys) in shots.items():
        if only and name not in only:
            continue
        scr = run(binary, args, cols, rows, keys, repo)
        render(scr, os.path.join(outdir, name + '.png'))
        print('wrote', name)
