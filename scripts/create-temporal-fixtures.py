"""Developer-only synthetic temporal corpus. No game screenshots are claimed."""
import json, math
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

root=Path('tests/fixtures/temporal');root.mkdir(parents=True,exist_ok=True)
font=ImageFont.truetype('C:/Windows/Fonts/msyh.ttc',32)
scenarios=[]
base=[('Health 100',80,100),('Quest complete',80,170),('游戏 OCR Test',80,240)]
names=['static-dialog','dynamic-background','typewriter','replacement','hud-digit','new-region','removed-region','scroll','move-resize','translucent','mixed-cjk','scene-switch','generation-reset','fade','moving-shadow','textured-background']
for name in names:
    frames=[]
    for n in range(6):
        lines=list(base);w,h=1280,720;dynamic=name in ['dynamic-background','translucent']
        image=Image.new('RGB',(w,h),(24,32,48))
        if dynamic:
            # Smooth moving illumination, with a translucent fixed UI panel.
            pixels=image.load()
            for x in range(w):
                c=20+int(10*(1+math.sin(x/250+n*.22)))
                for y in range(h):pixels[x,y]=(c,c+8,c+20)
        if name=='textured-background':
            import numpy as np
            rng=np.random.default_rng(200+n)
            texture=rng.integers(12,55,size=(h,w,1),dtype=np.uint8)
            image=Image.fromarray(np.repeat(texture,3,axis=2),'RGB')
        if n>=3:
            if name=='typewriter':lines[1]=('Quest '+['sta','start','started'][n-3],80,170)
            if name=='replacement':lines[1]=('Mission failed',80,170)
            if name=='hud-digit':lines[0]=('Health 101',80,100)
            if name=='new-region':lines.append(('New message',850,450))
            if name=='removed-region':lines.pop(1)
            if name=='scroll':lines=[(t,x,y+40)for t,x,y in lines]
            if name=='move-resize':w,h=1440,900;image=image.resize((w,h));lines=[(t,x+100,y+60)for t,x,y in lines]
            if name=='mixed-cjk':lines[2]=('游戏 繁體 日本語 101',80,240)
            if name=='scene-switch':lines=[('New scene',700,500)]
        if name=='translucent':
            layer=Image.new('RGBA',image.size);ImageDraw.Draw(layer).rectangle((50,70,620,320),fill=(10,20,30,100));image=Image.alpha_composite(image.convert('RGBA'),layer).convert('RGB')
        draw=ImageDraw.Draw(image);gt=[]
        for text,x,y in lines:
            if name=='moving-shadow':draw.text((x+3+n,y+3),text,font=font,fill=(0,0,0))
            color=(255,255,255)
            if name=='fade' and n>=3:color=([190,120,60][n-3],)*3
            draw.text((x,y),text,font=font,fill=color)
            gt.append({'text':text,'bbox':list(draw.textbbox((x,y),text,font=font))})
        file=f'{name}-{n}.png';image.save(root/file)
        frames.append({'image':file,'groundTruth':gt,'atMs':n*100,'generation':2 if name in ['scene-switch','generation-reset'] and n>=3 else 1})
    scenarios.append({'name':name,'source':'synthetic','frames':frames})
dense=json.loads(Path('tests/fixtures/small-text/corpus.json').read_text(encoding='utf-8-sig'))
for size in [2560,3840]:
    scene=next(s for s in dense['scenarios'] if s['width']==size)
    scenarios.append({'name':f'dense-{size}','source':'synthetic','parameters':{'preserveSmallText':True},'frames':[{'image':f'../small-text/dense-{size}.png','groundTruth':scene['groundTruth'],'atMs':n*100,'generation':1}for n in range(4)]})
(root/'corpus.json').write_text(json.dumps({'schemaVersion':1,'source':'synthetic; not real game footage','scenarios':scenarios},ensure_ascii=False,indent=2),encoding='utf-8')
print(f'Created {len(scenarios)} synthetic sequences')
