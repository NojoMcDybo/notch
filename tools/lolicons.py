import base64, io
"""Original-LoL-Symbole fuer die Notch neu erzeugen (src/lolicons.ts): laedt die Spieldateien von CommunityDragon.
Aufruf: python tools/lolicons.py"""
import os, tempfile, urllib.request
CD = "https://raw.communitydragon.org/latest/game/assets/"
TMP = tempfile.mkdtemp()
def get(path, name):
    f = os.path.join(TMP, name)
    req = urllib.request.Request(CD + path, headers={"User-Agent": "notch-lolicons"})
    with urllib.request.urlopen(req) as r, open(f, "wb") as out:
        out.write(r.read())
    return f
REPLAY = get("ux/spectator/replayatlas.png", "replayatlas.png")
GRUB = get("ux/minimap/icons/grub.png", "grub.png")
MELEE = get("characters/sru_orderminionmelee/hud/bluemelee_circle.png", "melee.png")
CANNON = get("characters/sru_orderminionsiege/hud/bluemechcannon_circle.png", "cannon.png")
SUPER = get("characters/sru_orderminionsuper/hud/bluemechmelee_circle.png", "super.png")
SUPER_R = get("characters/sru_chaosminionsuper/hud/redmechmelee_circle.png", "super_r.png")
OUT = os.path.join(os.path.dirname(__file__), "..", "src", "lolicons.ts")
from PIL import Image
atlas = Image.open(REPLAY).convert('RGBA')
def crop(b, pad=1):
    return atlas.crop((b[0]-pad, b[1]-pad, b[2]+1+pad, b[3]+1+pad))
def url(im):
    buf = io.BytesIO(); im.save(buf, 'PNG', optimize=True)
    return 'data:image/png;base64,' + base64.b64encode(buf.getvalue()).decode()
def mask(im, lum=False):
    im = im.convert('RGBA'); out = Image.new('RGBA', im.size)
    px = im.load(); o = out.load()
    mx = 1
    if lum:
        mx = max((0.3*r+0.59*g+0.11*b) for r,g,b,a in im.get_flattened_data() if a > 100) or 1
    for y in range(im.height):
        for x in range(im.width):
            r,g,b,a = px[x,y]
            if lum:
                l = (0.3*r+0.59*g+0.11*b)/mx
                l = max(0.0, min(1.0, (l-0.25)/0.6))
                a = int(a*l)
            o[x,y] = (255,255,255,a)
    return out
color = {
  'dragon': [896,203,917,221], 'elder': [922,200,939,222], 'infernal': [943,202,959,221], 'cloud': [963,202,979,221],
  'ocean': [983,203,999,220], 'mountain': [1003,202,1019,221], 'chemtech': [846,206,863,224], 'hextech': [871,207,888,225],
  'coins': [962,228,993,251], 'sword': [891,231,910,248],
}
masks = { 'tower': [956,255,972,271], 'inhib': [929,255,948,274], 'baron': [876,255,895,272], 'herald': [537,255,564,274] }
lums = { 'goldm': [962,228,993,251], 'killsm': [891,231,910,248] }
lines = []
for k,b in color.items(): lines.append(f'  {k}: "{url(crop(b))}",')
for k,b in masks.items(): lines.append(f'  {k}: "{url(mask(crop(b)))}",')
for k,b in lums.items(): lines.append(f'  {k}: "{url(mask(crop(b), True))}",')
g = Image.open(GRUB).convert('RGBA'); g.thumbnail((32,32), Image.LANCZOS)
lines.append(f'  grubs: "{url(g)}",')
for k,f in [('melee',MELEE),('cannon',CANNON),('super',SUPER),('superRed',SUPER_R)]:
    im = Image.open(f).convert('RGBA').resize((56,56), Image.LANCZOS); lines.append(f'  {k}: "{url(im)}",')
src = '''/**
 * Original-Symbole aus League of Legends (CommunityDragon: game/assets/ux/spectator/replayatlas.png — dieselben wie im
 * Zuschauer-Scoreboard der Übertragung —, minimap/icons/grub.png und die Minion-Porträts aus characters/sru_…minion…/hud (Kreis)).
 * Farbige Symbole als Bild; Türme, Inhibitoren, Baron, Herald sowie Gold und Kills (…m) als Maske, die in der
 * Teamfarbe eingefärbt wird. Erzeugt mit einem Skript aus den Spieldateien, nicht von Hand bearbeiten.
 */
export const LOL = {
''' + '\n'.join(lines) + '''
} as const;
export type LolIcon = keyof typeof LOL;
/** als Maske gedacht (Farbe = currentColor) */
export const LOL_MASK = new Set<LolIcon>(["tower", "inhib", "baron", "herald", "goldm", "killsm"]);
'''
open(OUT, 'w', encoding='utf-8', newline='\n').write(src)
print(len(src))
