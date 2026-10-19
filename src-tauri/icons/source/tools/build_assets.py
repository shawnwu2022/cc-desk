#!/usr/bin/env python3
"""Rebuild approved CC Desk stacked-terminal vector and export local assets.
Requires Python 3 + Pillow and the official Inkscape command-line renderer.
This script never reads or alters the raster concept reference.
"""
from pathlib import Path
from math import sqrt
from io import BytesIO
import hashlib, json, struct, subprocess, zipfile
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
PALETTE = {"charcoal":"#35404B", "orange":"#FA8939", "white":"#FFFFFF"}
SIZES = [16,24,32,48,64,128,256,512,1024]
ICO_SIZES = [16,24,32,48,64,128,256]


def chevron_path(x, y, length, radius):
    """Expanded, round-capped 45-degree chevron; closed filled geometry."""
    q = radius / sqrt(2)
    tipx, tipy = x+length, y+length
    boty = y+2*length
    pts = (x+q,y-q, tipx+q,tipy-q, radius,radius,tipx+q,tipy+q,
           x+q,boty+q,radius,radius,x-q,boty-q,tipx-2*q,tipy,
           x-q,y+q,radius,radius,x+q,y-q)
    return ("M %.6f %.6f L %.6f %.6f A %.6f %.6f 0 0 1 %.6f %.6f "
            "L %.6f %.6f A %.6f %.6f 0 0 1 %.6f %.6f L %.6f %.6f "
            "L %.6f %.6f A %.6f %.6f 0 0 1 %.6f %.6f Z") % pts


def svg(size, back, front, chev, under, description):
    x,y,w,h,r = back
    fx,fy,fw,fh,fr = front
    ux,uy,uw,uh,ur = under
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}" role="img" aria-labelledby="title desc">
  <title id="title">CC Desk — stacked terminal icon</title>
  <desc id="desc">{description}</desc>
  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{PALETTE['orange']}"/>
  <rect x="{fx}" y="{fy}" width="{fw}" height="{fh}" rx="{fr}" fill="{PALETTE['charcoal']}"/>
  <path d="{chevron_path(*chev)}" fill="{PALETTE['white']}"/>
  <rect x="{ux}" y="{uy}" width="{uw}" height="{uh}" rx="{ur}" fill="{PALETTE['white']}"/>
</svg>
'''

master = svg(256, (54,34,182,152,22), (20,57,196,161,20), (63,105,33,8.5),
             (107,164.5,65,17,8.5),
             "Frontal rounded charcoal terminal with a white greater-than and underscore prompt, in front of an orange card offset up and right. Flat fills, transparent canvas.")
variants = {
 16: svg(16,(3,2,12,10,1.9),(1,3.5,12.5,10.5,1.8),(4.1,6.8,2.1,0.8),(7.6,10.2,3.6,1.6,0.8),
         "Optically tuned 16 pixel variant: fuller silhouette, stronger prompt and orange offset. Flat fills, transparent canvas."),
 24: svg(24,(4.75,3,17.75,14.75,2.8),(1.5,5.25,19,15.75,2.9),(5.8,9.8,3.2,1.25),(10.4,15,6,2.5,1.25),
         "Optically tuned 24 pixel variant. Flat fills, transparent canvas."),
 32: svg(32,(6.75,4,23.25,19.5,3.1),(2,7,25.25,21,3.3),(7.5,13,4.4,1.65),(13.6,20.5,8.2,3.3,1.65),
         "Optically tuned 32 pixel variant. Flat fills, transparent canvas.")
}
(ROOT/'svg'/'ccdesk-master.svg').write_text(master)
for n,data in variants.items():
    (ROOT/'svg'/f'ccdesk-{n}.svg').write_text(data)

for n in SIZES:
    source = ROOT/'svg'/(f'ccdesk-{n}.svg' if n in variants else 'ccdesk-master.svg')
    out = ROOT/'png'/f'ccdesk-{n}.png'
    subprocess.run(['inkscape',str(source),'--export-type=png',f'--export-filename={out}',
                    f'--export-width={n}',f'--export-height={n}','--export-background-opacity=0'],
                   check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    im=Image.open(out).convert('RGBA')
    im.save(out, optimize=True)


def dib_frame(im):
    """32-bit BGRA ICO DIB with bottom-up XOR pixels + 1-bit AND mask."""
    im = im.convert('RGBA')
    w,h=im.size
    raw=im.tobytes('raw','BGRA',0,-1)
    stride=((w+31)//32)*4
    mask=bytearray(stride*h)
    a=im.getchannel('A')
    for row in range(h):
        for col in range(w):
            if a.getpixel((col,h-1-row))==0:
                mask[row*stride+col//8] |= 1 << (7-col%8)
    header=struct.pack('<IiiHHIIiiII',40,w,h*2,1,32,0,len(raw)+len(mask),0,0,0,0)
    return header+raw+mask

frames=[]
for n in ICO_SIZES:
    source=ROOT/'png'/f'ccdesk-{n}.png'
    frames.append((n, source.read_bytes() if n==256 else dib_frame(Image.open(source))))
offset=6+16*len(frames)
entries=[]
for n,payload in frames:
    entries.append(struct.pack('<BBBBHHII',0 if n==256 else n,0 if n==256 else n,0,0,1,32,len(payload),offset))
    offset+=len(payload)
(ROOT/'windows'/'ccdesk.ico').write_bytes(struct.pack('<HHH',0,1,len(frames))+b''.join(entries)+b''.join(p for _,p in frames))

FONTPATH='/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'
BOLDPATH='/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf'
def font(n,bold=False): return ImageFont.truetype(BOLDPATH if bold else FONTPATH,n)

def paste_icon(canvas,n,center,scale=1):
    im=Image.open(ROOT/'png'/f'ccdesk-{n}.png').convert('RGBA')
    if scale!=1: im=im.resize((n*scale,n*scale),Image.Resampling.NEAREST)
    canvas.alpha_composite(im,(int(center[0]-im.width//2),int(center[1]-im.height//2)))

W,H=1200,820
board=Image.new('RGBA',(W,H),'#F4F5F7')
d=ImageDraw.Draw(board)
d.rectangle((600,0,1199,H-1),fill='#1D242C')
for base,fg,muted,label in ((0,'#26323D','#5E6A76','LIGHT / #F4F5F7'),(600,'#F4F5F7','#B3BCC6','DARK / #1D242C')):
    d.text((base+36,28),'CC DESK / STACKED TERMINAL',font=font(20,True),fill=fg)
    d.text((base+36,62),label,font=font(13),fill=muted)
    paste_icon(board,128,(base+160,168))
    d.text((base+278,124),'Flat vector rebuild',font=font(18,True),fill=fg)
    d.text((base+278,156),'Transparent RGBA exports',font=font(14),fill=muted)
    d.text((base+278,180),'128 × 128 source, shown at 1×',font=font(13),fill=muted)
    d.line((base+36,260,base+564,260),fill=muted,width=1)
    d.text((base+36,279),'OPTICALLY TUNED SMALL SIZES',font=font(15,True),fill=fg)
    d.text((base+36,307),'1× means exact source pixels at 100% image zoom',font=font(12),fill=muted)
    for row,n in enumerate((16,24,32)):
        cy=395+row*142
        scale={16:6,24:4,32:3}[n]
        d.text((base+36,cy-47),f'{n} × {n}',font=font(18,True),fill=fg)
        paste_icon(board,n,(base+178,cy))
        d.text((base+159,cy+57),'1×',font=font(14),fill=muted)
        paste_icon(board,n,(base+402,cy),scale)
        d.text((base+318,cy+57),f'Nearest-neighbor {scale}×',font=font(14),fill=muted)
    d.text((base+36,780),'Pixel preview only; not an installed Windows app test',font=font(12),fill=muted)
board.convert('RGB').save(ROOT/'previews'/'small-size-preview.png',optimize=True)

# A compact board specifically for inspecting the 16/24/32 output pixels.
board2=Image.new('RGBA',(960,280),'#F4F5F7')
d2=ImageDraw.Draw(board2)
d2.rectangle((480,0,959,279),fill='#1D242C')
for base,fg in ((0,'#26323D'),(480,'#F4F5F7')):
    d2.text((base+20,16),'16 / 24 / 32 px · actual size + nearest 4×',font=font(14,True),fill=fg)
    for i,n in enumerate((16,24,32)):
        cx=base+80+i*160
        paste_icon(board2,n,(cx,75))
        d2.text((cx-19,100),f'{n}px',font=font(13),fill=fg)
        paste_icon(board2,n,(cx,192),4)
board2.convert('RGB').save(ROOT/'previews'/'small-size-strip.png',optimize=True)

# Deterministic QA: output dimensions, transparency, clipping, ICO frame equality.
qa={"renderer":subprocess.run(['inkscape','--version'],capture_output=True,text=True,check=True).stdout.strip(),
    "palette":PALETTE,"png":{},"ico":{},"limitations":["No installed Windows shell or application runtime test.","Rendering may vary slightly between SVG renderers and OS scaling."]}
for n in SIZES:
    im=Image.open(ROOT/'png'/f'ccdesk-{n}.png').convert('RGBA')
    assert im.size==(n,n)
    alpha=im.getchannel('A')
    bbox=alpha.getbbox()
    assert bbox and bbox[0]>0 and bbox[1]>0 and bbox[2]<n and bbox[3]<n, (n,bbox)
    assert alpha.getextrema()==(0,255)
    assert all(alpha.getpixel(p)==0 for p in ((0,0),(n-1,0),(0,n-1),(n-1,n-1)))
    qa['png'][str(n)]={"size":[n,n],"mode":im.mode,"alpha_bbox":list(bbox),"transparent_canvas":True,"edge_clipping":False}
ico=Image.open(ROOT/'windows'/'ccdesk.ico')
assert ico.ico.sizes()=={(n,n) for n in ICO_SIZES}
for n in ICO_SIZES:
    a=ico.ico.getimage((n,n)).convert('RGBA')
    b=Image.open(ROOT/'png'/f'ccdesk-{n}.png').convert('RGBA')
    assert a.tobytes()==b.tobytes(), f'ICO/PNG mismatch at {n}'
qa['ico']={"sizes":ICO_SIZES,"rgba_matches_png":True,"frames_16_to_128":"32-bit BGRA DIB + AND transparency masks","frame_256":"PNG"}
(ROOT/'qa.json').write_text(json.dumps(qa,indent=2)+'\n')

readme='''# CC Desk — stacked terminal icon

Standalone, hand-authored SVG reconstruction of the approved refined stacked-terminal concept. No application repository, release, system setting or original raster reference was changed.

## Deliverables
- `svg/ccdesk-master.svg`: 256 × 256 vector master, intended for 48 px and larger exports
- `svg/ccdesk-16.svg`, `ccdesk-24.svg`, `ccdesk-32.svg`: separately tuned vector variants for small display sizes
- `png/ccdesk-{size}.png`: transparent 8-bit RGBA PNGs at 16, 24, 32, 48, 64, 128, 256, 512 and 1024 px
- `windows/ccdesk.ico`: 16, 24, 32, 48, 64, 128 and 256 px frames, using the corresponding PNG geometry at every size
- `previews/small-size-preview.png`: labeled light/dark board, with native-size icons and nearest-neighbor enlargements
- `previews/small-size-strip.png`: compact native-size / nearest-neighbor 4× pixel-inspection strip
- `qa.json`: renderer details and automated export checks
- `tools/build_assets.py`: local, repeatable export script; requires Python 3, Pillow and Inkscape

## Design specification
- Orange: `#FA8939`
- Charcoal: `#35404B`, lifted slightly to retain an edge on dark backgrounds
- Prompt: `#FFFFFF`
- Transparent canvas; flat fills only. No gradients, shadows, embedded bitmap, external font or linked asset dependencies in the SVGs
- Frontal rounded terminal card; orange rear card offset toward the top-right
- White greater-than and underscore are vector geometry, not text glyphs
- Master viewBox: `0 0 256 256`
- Master silhouette: x = 20–236, y = 34–218; front card 196 × 161, rear card 182 × 152
- Master front radius: 20; rear radius: 22; prompt thickness: 17
- At 16/24/32 px the silhouette is slightly fuller, prompt weight/placement and offsets are tuned to those pixel grids. Use the matching small SVG/PNG instead of downscaling the master for those exact sizes

## Verification and limitations
- All 9 PNGs are RGBA, have fully transparent canvas pixels, and have empty space at every canvas edge; no clipping
- All 7 ICO frames were decoded and compared pixel-for-pixel against the matching PNG exports
- ICO uses 32-bit BGRA DIB data plus 1-bit AND transparency masks for 16–128 px, with a PNG-compressed 256 px frame
- Preview boards composite the exported pixels onto light `#F4F5F7` and dark `#1D242C` backgrounds. The marked 1× views are native pixels in the image file. View the board at 100% zoom to inspect true image-pixel size
- No installed Windows app, taskbar, tray, Start menu or executable resource test was performed. Windows integration and release packaging are separate steps
- SVG rasterization and fractional OS scaling may vary by renderer. The included PNGs and ICO fix the intended pixels for the listed sizes

## Rebuild
Run `python tools/build_assets.py` from any working directory. The script rewrites this standalone asset folder only. No package installation or network access is performed.
'''
(ROOT/'README.md').write_text(readme)

files=sorted(p for p in ROOT.rglob('*') if p.is_file() and p.suffix!='.zip' and p.name!='SHA256SUMS.txt')
(ROOT/'SHA256SUMS.txt').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(ROOT).as_posix()}\n' for p in files))
zip_path=ROOT.parent/'ccdesk-icon-refined.zip'
with zipfile.ZipFile(zip_path,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
    for p in sorted(ROOT.rglob('*')):
        if p.is_file(): z.write(p,arcname=Path(ROOT.name)/p.relative_to(ROOT))
print(json.dumps({"root":str(ROOT),"zip":str(zip_path),"files":len(files)+1,"sizes":SIZES,"qa":qa},indent=2))
