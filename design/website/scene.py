"""The hero scene, drawn in isometric projection: an island garden, the app
floating over it as a sheet with its layers lifted off, clouds at several
depths. Writes art.html: every layer is its own <svg>, so each can be moved
on its own. `layers.sh` cuts them into the pictures the website moves.

    python3 scene.py [out.html]

The page takes ?only=<layer> to show one layer, ?still=1 to stop the motion
and ?t=<seconds> to stop it at a moment."""
import math, os, sys, base64

HERE = os.path.dirname(os.path.abspath(__file__))
W, H = 900, 940            # the art's canvas
S = 44.0                   # pixels to one unit of the world
C30 = math.cos(math.radians(30))
CX, CY = 470.0, 640.0      # where the middle of the island's top is drawn


def iso(x, y, z=0.0):
    """World (x right-down, y left-down, z up) to the canvas."""
    return (CX + (x - y) * C30 * S, CY + (x + y) * 0.5 * S - z * S)


def pts(*p):
    return " ".join("%.1f,%.1f" % q for q in p)


def b64(name):
    return "data:image/png;base64," + base64.b64encode(open(os.path.join(HERE, name), "rb").read()).decode()


# ------------------------------------------------------------------ pieces
def defs():
    return """
<defs>
  <radialGradient id="grass" cx="42%" cy="36%" r="75%">
    <stop offset="0" stop-color="#DDF0AE"/><stop offset=".55" stop-color="#B4D57A"/><stop offset="1" stop-color="#8FBB5A"/>
  </radialGradient>
  <linearGradient id="rim" x1="0" x2="1"><stop offset="0" stop-color="#9CC463"/><stop offset=".55" stop-color="#7FAA49"/><stop offset="1" stop-color="#5D8733"/></linearGradient>
  <linearGradient id="soil" x1="0" x2="1"><stop offset="0" stop-color="#C79A72"/><stop offset=".5" stop-color="#AA7D58"/><stop offset="1" stop-color="#83603F"/></linearGradient>
  <linearGradient id="rock" x1="0" y1="0" x2="0.35" y2="1"><stop offset="0" stop-color="#A47C5C"/><stop offset=".6" stop-color="#7C5C44"/><stop offset="1" stop-color="#5A4231"/></linearGradient>
  <linearGradient id="rockLight" x1="0" x2="1"><stop offset="0" stop-color="#fff" stop-opacity=".22"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
  <linearGradient id="rockDark" x1="0" x2="1"><stop offset="0" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".22"/></linearGradient>
  <radialGradient id="patio" cx="45%" cy="40%" r="70%"><stop offset="0" stop-color="#FFFCF1"/><stop offset="1" stop-color="#F3EAD0"/></radialGradient>
  <linearGradient id="cypress" x1="0" x2="1"><stop offset="0" stop-color="#7FB04C"/><stop offset=".55" stop-color="#55883A"/><stop offset="1" stop-color="#3C6A2C"/></linearGradient>
  <radialGradient id="leaf" cx="35%" cy="30%" r="80%"><stop offset="0" stop-color="#CFEA93"/><stop offset=".6" stop-color="#8DBB52"/><stop offset="1" stop-color="#5F8E38"/></radialGradient>
  <linearGradient id="trunk" x1="0" x2="1"><stop offset="0" stop-color="#A27C58"/><stop offset="1" stop-color="#6F5038"/></linearGradient>
  <radialGradient id="shade" cx="50%" cy="50%" r="50%"><stop offset="0" stop-color="#1F2A0C" stop-opacity=".5"/><stop offset=".6" stop-color="#1F2A0C" stop-opacity=".24"/><stop offset="1" stop-color="#1F2A0C" stop-opacity="0"/></radialGradient>
  <linearGradient id="water" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#BFE6FF"/><stop offset="1" stop-color="#6DB7EC"/></linearGradient>
  <linearGradient id="cloud" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset=".6" stop-color="#F6F9FD"/><stop offset="1" stop-color="#D5E2F2"/></linearGradient>
  <linearGradient id="slabTop" x1="0" x2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#EFE9DB"/></linearGradient>
  <linearGradient id="slabSide" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#DDD6C4"/><stop offset="1" stop-color="#BFB7A3"/></linearGradient>
  <linearGradient id="gloss" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".0"/><stop offset=".45" stop-color="#fff" stop-opacity=".0"/><stop offset=".5" stop-color="#fff" stop-opacity=".16"/><stop offset=".62" stop-color="#fff" stop-opacity="0"/></linearGradient>
</defs>"""


def island():
    R = 4.5
    rx, ry = R * S * math.sqrt(1.5), R * S * math.sqrt(0.5)
    L, Rr = CX - rx, CX + rx
    h1, h2 = 15, 42
    y1, y2 = CY + h1, CY + h1 + h2
    band = lambda ya, yb, fill: (
        f'<path d="M{L:.1f},{ya:.1f} A{rx:.1f},{ry:.1f} 0 0 0 {Rr:.1f},{ya:.1f} L{Rr:.1f},{yb:.1f} '
        f'A{rx:.1f},{ry:.1f} 0 0 1 {L:.1f},{yb:.1f} Z" fill="{fill}"/>')
    tip = (CX + 14, y2 + 232)
    rock_d = (f"M{L:.1f},{y2:.1f} A{rx:.1f},{ry:.1f} 0 0 0 {Rr:.1f},{y2:.1f} "
              f"C{Rr - 6:.1f},{y2 + 70:.1f} {CX + 168:.1f},{y2 + 150:.1f} {CX + 70:.1f},{y2 + 196:.1f} "
              f"Q{tip[0] + 16:.1f},{tip[1] + 6:.1f} {tip[0] - 22:.1f},{tip[1] - 26:.1f} "
              f"C{CX - 128:.1f},{y2 + 150:.1f} {L + 10:.1f},{y2 + 84:.1f} {L:.1f},{y2:.1f} Z")
    out = ['<g id="rock">', f'<path d="{rock_d}" fill="url(#rock)"/>']
    # the lit side and the side in shade, and strata to give the rock weight
    out.append(f'<path d="{rock_d}" fill="url(#rockLight)" clip-path="inset(0 55% 0 0)"/>')
    out.append(f'<path d="{rock_d}" fill="url(#rockDark)"/>')
    for k, (dy, op) in enumerate([(52, .16), (104, .13), (150, .1)]):
        w = rx * (1 - dy / 250.0)
        out.append(f'<path d="M{CX - w * .92:.1f},{y2 + dy:.1f} Q{CX:.1f},{y2 + dy + ry * .5 * (1 - dy / 260):.1f} {CX + w * .86:.1f},{y2 + dy - 6:.1f}" '
                   f'fill="none" stroke="#3A2A1E" stroke-opacity="{op}" stroke-width="2"/>')
    for (ax, ay), (bx, by), (cx_, cy_), col, op in [
            ((CX - 150, y2 + 70), (CX - 70, y2 + 96), (CX - 96, y2 + 150), "#fff", .10),
            ((CX - 40, y2 + 104), (CX + 30, y2 + 96), (CX + 4, y2 + 176), "#fff", .07),
            ((CX + 70, y2 + 92), (CX + 150, y2 + 70), (CX + 96, y2 + 150), "#000", .10),
            ((CX - 200, y2 + 40), (CX - 150, y2 + 70), (CX - 190, y2 + 84), "#fff", .12)]:
        out.append(f'<polygon points="{ax:.1f},{ay:.1f} {bx:.1f},{by:.1f} {cx_:.1f},{cy_:.1f}" fill="{col}" opacity="{op}"/>')
    out.append('</g>')
    out.append(band(y1, y2, "url(#soil)"))
    out.append(f'<path d="M{L:.1f},{y1 + 13:.1f} A{rx:.1f},{ry:.1f} 0 0 0 {Rr:.1f},{y1 + 13:.1f}" fill="none" stroke="#6B4A30" stroke-opacity=".28" stroke-width="2"/>')
    out.append(band(CY, y1, "url(#rim)"))
    # grass hanging over the rim, so the edge is soft
    for ang in (200, 222, 246, 268, 292, 316, 338):
        a = math.radians(ang)
        px, py = CX + rx * math.cos(a) * -1, CY + ry * abs(math.sin(a)) + h1 - 2
        out.append(f'<ellipse cx="{px:.1f}" cy="{py:.1f}" rx="13" ry="8" fill="#7FAA49"/>')
    out.append(f'<ellipse cx="{CX}" cy="{CY}" rx="{rx:.1f}" ry="{ry:.1f}" fill="url(#grass)"/>')
    out.append(f'<ellipse cx="{CX}" cy="{CY}" rx="{rx - 3:.1f}" ry="{ry - 2:.1f}" fill="none" stroke="#fff" stroke-opacity=".35" stroke-width="2"/>')
    return "\n".join(out), (rx, ry)


def patio():
    Rp = 2.15
    rx, ry = Rp * S * math.sqrt(1.5), Rp * S * math.sqrt(0.5)
    a, b, c, d = C30 * S, 0.5 * S, -C30 * S, 0.5 * S
    k = 2.9 / 240.0
    mark = open(os.path.join(HERE, "..", "logo", "mark.svg")).read()
    inner = mark[mark.index(">", mark.index("<svg")) + 1: mark.rindex("</svg>")]
    return (f'<ellipse cx="{CX}" cy="{CY + 5}" rx="{rx + 7:.1f}" ry="{ry + 4:.1f}" fill="#6F9440" opacity=".5"/>'
            f'<ellipse cx="{CX}" cy="{CY}" rx="{rx + 6:.1f}" ry="{ry + 3.5:.1f}" fill="#E6D9B4"/>'
            f'<ellipse cx="{CX}" cy="{CY - 2}" rx="{rx:.1f}" ry="{ry:.1f}" fill="url(#patio)"/>'
            f'<ellipse cx="{CX}" cy="{CY - 2}" rx="{rx * .8:.1f}" ry="{ry * .8:.1f}" fill="none" stroke="#EBC034" stroke-opacity=".55" stroke-width="2.5" stroke-dasharray="3 7" stroke-linecap="round"/>'
            f'<g transform="matrix({a:.3f} {b:.3f} {c:.3f} {d:.3f} {CX} {CY - 2}) scale({k:.5f}) translate(-120 -120)">{inner}</g>')


def tile(x, y, r=.34, fill="#F7F0DC", edge="#E0D3AC"):
    """A lozenge tile lying on the ground."""
    p = [iso(x - r, y - r), iso(x + r, y - r), iso(x + r, y + r), iso(x - r, y + r)]
    q = [(a, b + 3) for a, b in p]
    return f'<polygon points="{pts(*q)}" fill="{edge}"/><polygon points="{pts(*p)}" fill="{fill}"/>'


def shadow_at(x, y, rx=20, ry=9, op=1.0):
    px, py = iso(x, y)
    return f'<ellipse cx="{px:.1f}" cy="{py:.1f}" rx="{rx}" ry="{ry}" fill="url(#shade)" opacity="{op}"/>'


def cypress(x, y, s=1.0):
    px, py = iso(x, y)
    return (shadow_at(x + .25, y + .15, 22 * s, 9 * s)
            + f'<rect x="{px - 3 * s:.1f}" y="{py - 14 * s:.1f}" width="{6 * s:.1f}" height="{16 * s:.1f}" rx="2" fill="url(#trunk)"/>'
            + f'<path d="M{px:.1f},{py - 8 * s:.1f} C{px - 17 * s:.1f},{py - 30 * s:.1f} {px - 13 * s:.1f},{py - 96 * s:.1f} {px:.1f},{py - 118 * s:.1f} '
              f'C{px + 13 * s:.1f},{py - 96 * s:.1f} {px + 17 * s:.1f},{py - 30 * s:.1f} {px:.1f},{py - 8 * s:.1f} Z" fill="url(#cypress)"/>'
            + f'<path d="M{px - 3 * s:.1f},{py - 100 * s:.1f} C{px - 10 * s:.1f},{py - 78 * s:.1f} {px - 11 * s:.1f},{py - 46 * s:.1f} {px - 7 * s:.1f},{py - 26 * s:.1f}" fill="none" stroke="#fff" stroke-opacity=".28" stroke-width="{3 * s:.1f}" stroke-linecap="round"/>')


def tree(x, y, s=1.0):
    px, py = iso(x, y)
    c = lambda dx, dy, r: f'<circle cx="{px + dx * s:.1f}" cy="{py + dy * s:.1f}" r="{r * s:.1f}" fill="url(#leaf)"/>'
    return (shadow_at(x + .3, y + .2, 26 * s, 11 * s)
            + f'<rect x="{px - 3.5 * s:.1f}" y="{py - 30 * s:.1f}" width="{7 * s:.1f}" height="{32 * s:.1f}" rx="3" fill="url(#trunk)"/>'
            + c(-13, -42, 18) + c(13, -40, 17) + c(0, -60, 22))


def bush(x, y, s=1.0):
    px, py = iso(x, y)
    return (shadow_at(x + .2, y + .1, 20 * s, 8 * s, .8)
            + f'<circle cx="{px - 8 * s:.1f}" cy="{py - 8 * s:.1f}" r="{11 * s:.1f}" fill="url(#leaf)"/>'
            + f'<circle cx="{px + 7 * s:.1f}" cy="{py - 10 * s:.1f}" r="{13 * s:.1f}" fill="url(#leaf)"/>')


def stone(x, y, s=1.0):
    px, py = iso(x, y)
    return (f'<ellipse cx="{px + 2:.1f}" cy="{py + 2:.1f}" rx="{11 * s}" ry="{5 * s}" fill="#1F2A0C" opacity=".18"/>'
            f'<path d="M{px - 9 * s:.1f},{py:.1f} L{px - 5 * s:.1f},{py - 8 * s:.1f} L{px + 4 * s:.1f},{py - 9 * s:.1f} L{px + 10 * s:.1f},{py - 2 * s:.1f} L{px + 6 * s:.1f},{py + 3 * s:.1f} L{px - 5 * s:.1f},{py + 3 * s:.1f} Z" fill="#D9D4C7"/>'
            f'<path d="M{px + 4 * s:.1f},{py - 9 * s:.1f} L{px + 10 * s:.1f},{py - 2 * s:.1f} L{px + 6 * s:.1f},{py + 3 * s:.1f} L{px + 1 * s:.1f},{py - 3 * s:.1f} Z" fill="#B9B3A4"/>')


def pond(x, y):
    px, py = iso(x, y)
    return (f'<ellipse cx="{px:.1f}" cy="{py + 2:.1f}" rx="50" ry="27" fill="#7FA94A"/>'
            f'<ellipse cx="{px:.1f}" cy="{py:.1f}" rx="46" ry="24" fill="url(#water)"/>'
            f'<path d="M{px - 26:.1f},{py - 6:.1f} q10,-9 26,-8" fill="none" stroke="#fff" stroke-opacity=".8" stroke-width="3" stroke-linecap="round"/>'
            f'<ellipse cx="{px + 16:.1f}" cy="{py + 8:.1f}" rx="9" ry="4" fill="#fff" opacity=".35"/>')


def flowers():
    import random
    random.seed(11)
    out = []
    for _ in range(26):
        ang, rad = random.uniform(0, math.tau), random.uniform(2.6, 4.2)
        x, y = rad * math.cos(ang), rad * math.sin(ang)
        px, py = iso(x, y)
        col = random.choice(["#FFE27A", "#FFFFFF", "#FFC9B5", "#FFE27A"])
        out.append(f'<circle cx="{px:.1f}" cy="{py:.1f}" r="2.6" fill="{col}"/>')
    return "".join(out)


def cloud(x, y, s=1.0, uid="c"):
    """A cloud with some body to it: lit from above, a belly in shade."""
    g = f'<linearGradient id="{uid}" gradientUnits="userSpaceOnUse" x1="0" y1="{y - 60 * s:.1f}" x2="0" y2="{y + 34 * s:.1f}"><stop offset="0" stop-color="#FFFFFF"/><stop offset=".55" stop-color="#F7FAFE"/><stop offset="1" stop-color="#CFDDEF"/></linearGradient>'
    c = lambda dx, dy, r: f'<circle cx="{x + dx * s:.1f}" cy="{y + dy * s:.1f}" r="{r * s:.1f}"/>'
    body = (c(-62, 6, 30) + c(-24, -16, 40) + c(26, -24, 46) + c(70, 2, 32)
            + f'<rect x="{x - 92 * s:.1f}" y="{y - 2 * s:.1f}" width="{194 * s:.1f}" height="{36 * s:.1f}" rx="{18 * s:.1f}"/>')
    return f'<defs>{g}</defs><g fill="url(#{uid})">{body}</g>'


def slab():
    """The app: a sheet standing in the air, as thick as a board."""
    x0, x1, y, z0, z1, t = -3.1, 3.1, 0.2, 1.75, 9.45, 0.3
    TL, TR, BR, BL = iso(x0, y, z1), iso(x1, y, z1), iso(x1, y, z0), iso(x0, y, z0)
    back = lambda p: (p[0] + C30 * S * t, p[1] - 0.5 * S * t)
    w, h = TR[0] - TL[0], BR[1] - TL[1]
    return (f'<polygon points="{pts(TL, TR, back(TR), back(TL))}" fill="url(#slabTop)"/>'
            f'<polygon points="{pts(TR, back(TR), back(BR), BR)}" fill="url(#slabSide)"/>'
            f'<image href="{b64("tex-sheet.png")}" x="{TL[0]:.1f}" y="{TL[1]:.1f}" width="{w:.1f}" height="{h:.1f}" preserveAspectRatio="none"/>'
            f'<polygon points="{pts(TL, TR, BR, BL)}" fill="url(#gloss)"/>'
            f'<polygon points="{pts(TL, TR, BR, BL)}" fill="none" stroke="#1E1D1A" stroke-opacity=".10" stroke-width="1"/>'), (TL, TR, BR, BL)


def card(tex, x0, z1, w, h, y):
    """A piece of the app lifted off the sheet: `w` by `h` in pixels of its own face."""
    TL = iso(x0, y, z1)
    return f'<image href="{b64(tex)}" x="{TL[0]:.1f}" y="{TL[1]:.1f}" width="{w * C30:.1f}" height="{h + w * 0.5:.1f}" preserveAspectRatio="none"/>'


def plane():
    return ('<path d="M0,0 C80,-30 150,-34 214,-78" fill="none" stroke="#8FA9CC" stroke-opacity=".65" stroke-width="2" stroke-dasharray="2 9" stroke-linecap="round"/>'
            '<g transform="translate(226 -86) rotate(-14)">'
            '<polygon points="0,0 -62,-2 -36,13" fill="#FFFFFF"/>'
            '<polygon points="0,0 -36,13 -47,36" fill="#DDE5F0"/>'
            '<polygon points="0,0 -36,13 -30,24" fill="#B9C6DA"/></g>')


def chunk(x, y, s=1.0):
    return (f'<g transform="translate({x} {y}) scale({s})">'
            '<polygon points="-16,-6 -4,-14 14,-10 18,2 6,14 -10,10" fill="#8C6A4F"/>'
            '<polygon points="-16,-6 -4,-14 14,-10 2,-3" fill="#B08A68"/>'
            '<polygon points="14,-10 18,2 6,14 2,-3" fill="#6B4E3A"/></g>')


# ------------------------------------------------------------------- layers
def layers():
    isl, (rx, ry) = island()
    sl, (TL, TR, BR, BL) = slab()
    sx, sy = iso(0, 0.9)
    L = []
    L.append(("cloud-far-1", cloud(150, 150, .62, "cf1"), "filter:blur(1.2px);opacity:.9"))
    L.append(("cloud-far-2", cloud(735, 96, .8, "cf2"), "filter:blur(.8px);opacity:.95"))
    L.append(("island",
              chunk(CX - rx - 32, CY + 168, 1.0) + chunk(CX + rx - 6, CY + 196, .7) + chunk(CX - 150, CY + 262, .5)
              + isl + patio() + flowers()
              + tile(.15, 2.8, .3) + tile(.95, 3.05, .3, "#F2D261", "#C9A227") + tile(1.75, 3.3, .3)
              + cypress(0.45, -4.35, .88) + cypress(2.0, -3.6, 1.15) + tree(2.75, -2.85, 1.05) + tree(3.65, -1.25, .82)
              + stone(-3.6, .9, 1) + stone(-3.0, 1.7, .7), ""))
    L.append(("shadow",
              f'<ellipse cx="{sx:.1f}" cy="{sy:.1f}" rx="176" ry="46" transform="rotate(30 {sx:.1f} {sy:.1f})" fill="url(#shade)"/>', ""))
    L.append(("cloud-mid", cloud(800, 318, .9, "cm1"), "filter:drop-shadow(0 14px 16px rgba(70,90,130,.18))"))
    L.append(("sheet", sl, "filter:drop-shadow(16px 26px 26px rgba(40,34,20,.26))"))
    L.append(("island-front", tree(-2.7, 3.0, .95) + bush(-1.3, 3.7, 1.0) + bush(2.0, 3.9, .8) + pond(3.2, 2.2) + stone(2.5, 3.4, .8), ""))
    L.append(("card-diagram", card("tex-diagram.png", -5.75, 5.3, 262, 139.2, 2.3), "filter:drop-shadow(12px 20px 16px rgba(40,34,20,.28))"))
    L.append(("card-rail", card("tex-rail.png", 0.95, 7.55, 190, 95.7, 1.7), "filter:drop-shadow(12px 20px 16px rgba(40,34,20,.28))"))
    L.append(("card-edit", card("tex-edit.png", 1.15, 10.35, 110, 33.4, 1.25), "filter:drop-shadow(8px 14px 12px rgba(120,90,0,.35))"))
    L.append(("plane", f'<g transform="translate(70 372) scale(1.25)">{plane()}</g>', "filter:drop-shadow(0 10px 8px rgba(70,90,130,.22))"))
    L.append(("cloud-near-1", cloud(196, 668, 1.12, "cn1"), "filter:drop-shadow(0 18px 18px rgba(70,90,130,.22))"))
    L.append(("cloud-near-2", cloud(772, 742, .78, "cn2"), "filter:drop-shadow(0 14px 14px rgba(70,90,130,.2))"))
    return L


ANIM = {  # layer: (css animation shorthand)
    "island": "bob 9s ease-in-out infinite alternate",
    "island-front": "bob 9s ease-in-out infinite alternate",
    "shadow": "shade 6.4s ease-in-out infinite alternate",
    "sheet": "float 6.4s ease-in-out infinite alternate",
    "card-diagram": "floatA 5.2s ease-in-out -1.3s infinite alternate",
    "card-rail": "floatB 4.6s ease-in-out -2.1s infinite alternate",
    "card-edit": "floatC 3.8s ease-in-out -.6s infinite alternate",
    "cloud-far-1": "drift 46s linear infinite alternate",
    "cloud-far-2": "driftBack 58s linear infinite alternate",
    "cloud-mid": "driftBack 38s linear infinite alternate",
    "cloud-near-1": "drift 30s linear infinite alternate",
    "cloud-near-2": "driftBack 34s linear infinite alternate",
    "plane": "glide 11s ease-in-out infinite alternate",
}

CSS = """
.art{position:relative;width:%(W)dpx;height:%(H)dpx}
.art svg{position:absolute;left:0;top:0;width:%(W)dpx;height:%(H)dpx;overflow:visible;will-change:transform}
@keyframes bob{from{transform:translateY(-7px)}to{transform:translateY(7px)}}
@keyframes float{from{transform:translateY(-16px) rotate(-.6deg)}to{transform:translateY(12px) rotate(.5deg)}}
@keyframes shade{from{transform:scale(.86);opacity:.7}to{transform:scale(1.04);opacity:1}}
@keyframes floatA{from{transform:translate(-4px,-20px)}to{transform:translate(3px,10px)}}
@keyframes floatB{from{transform:translate(5px,-24px)}to{transform:translate(-3px,6px)}}
@keyframes floatC{from{transform:translate(2px,-26px)}to{transform:translate(-2px,2px)}}
@keyframes drift{from{transform:translateX(-46px)}to{transform:translateX(54px)}}
@keyframes driftBack{from{transform:translateX(40px)}to{transform:translateX(-50px)}}
@keyframes glide{from{transform:translate(-40px,30px) rotate(3deg)}to{transform:translate(70px,-34px) rotate(-3deg)}}
#shadow{transform-origin:%(sx).0fpx %(sy).0fpx}
#sheet{transform-origin:%(CX).0fpx %(sheet_cy).0fpx}
""" % dict(W=W, H=H, sx=iso(0, 0.9)[0], sy=iso(0, 0.9)[1], CX=CX, sheet_cy=CY - 250)


def art_html():
    out = ['<div class="art">']
    for name, body, style in layers():
        anim = ANIM.get(name)
        st = (style + ";" if style else "") + (f"animation:{anim}" if anim else "")
        out.append(f'<svg id="{name}" class="layer" viewBox="0 0 {W} {H}" style="{st}">{defs()}{body}</svg>')
    out.append("</div>")
    return "\n".join(out)


if __name__ == "__main__":
    html = f"""<!doctype html><meta charset="utf-8"><style>
html,body{{margin:0;background:transparent}}
{CSS}
</style><body>{art_html()}
<script>
const q=new URLSearchParams(location.search);
if(q.get('only'))document.querySelectorAll('.layer').forEach(l=>{{if(l.id!==q.get('only'))l.style.visibility='hidden'}});
if(q.get('t')!==null)document.querySelectorAll('.layer').forEach(l=>{{l.style.animationPlayState='paused';
  const d=parseFloat(getComputedStyle(l).animationDelay)||0;l.style.animationDelay=(d-parseFloat(q.get('t')))+'s'}});
if(q.get('still'))document.querySelectorAll('.layer').forEach(l=>l.style.animation='none');
</script></body>"""
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "art.html")
    open(out, "w").write(html)
    print("wrote", out, len(html) // 1024, "KB")
