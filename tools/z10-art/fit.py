import json, os, subprocess, sys, numpy as np
from PIL import Image
D=json.load(open('dom.json')); sx=1240/649
def score(st):
    a=np.asarray(Image.open(f'out/{st}-ref.png').convert('RGB')).astype(int)[:496]; b=np.asarray(Image.open(f'out/{st}-svg.png').convert('RGB')).astype(int)[:496]
    m=np.zeros(a.shape[:2],bool)
    # the deck and well floor round the keys: inside the wells, outside every cap
    for x0,y0,x1,y1 in [(21,59,640,226)]: m[int((y0+2)*sx):int((y1+2)*sx), int((x0-6)*sx):int((x1-6)*sx)]=True
    for k in D['keys']:
        x,y,w,h=k['box']; m[int((y+2)*sx)-1:int((y+2+h)*sx)+2, int((x-6)*sx)-1:int((x-6+w)*sx)+2]=False
    d=(a-b)[m]; return np.abs(d).mean(), d.mean()
print(score(sys.argv[1]))
