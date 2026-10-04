"""Geometry for the tug rope mark.

Usage: python scripts/rope_geometry.py <stroke-width> <out.json> [twist-step]

Prints a smooth rope path through the control points plus short diagonal
"twist" cuts along it. src-tauri/icons/source.svg uses step 0.85 / width 2.1;
src/components/TugMark.vue uses step 1.5 / width 2.6. After editing source.svg,
regenerate the PNG/ICO set with: npx tauri icon src-tauri/icons/source.svg -o src-tauri/icons
"""
import math, sys, json
P = [(4.6,19.8),(4.4,11.0),(6.4,4.9),(10.2,4.8),(12.0,8.6),(11.4,12.6),(12.8,15.4),(15.6,13.4),(17.0,10.2),(19.2,10.6),(19.9,14.2),(18.6,17.6),(20.4,20.0)]
def catmull(p0,p1,p2,p3,t):
    t2,t3=t*t,t*t*t
    return tuple(0.5*((2*p1[i])+(-p0[i]+p2[i])*t+(2*p0[i]-5*p1[i]+4*p2[i]-p3[i])*t2+(-p0[i]+3*p1[i]-3*p2[i]+p3[i])*t3) for i in range(2))
pts=[P[0]]+P+[P[-1]]
samples=[]
for i in range(1,len(pts)-2):
    for k in range(40):
        samples.append(catmull(pts[i-1],pts[i],pts[i+1],pts[i+2],k/40))
samples.append(P[-1])
# path d as polyline (dense, smooth enough)
d="M"+" L".join(f"{x:.2f} {y:.2f}" for x,y in samples)
W=float(sys.argv[1]) if len(sys.argv)>1 else 2.1
step=0.85; ang=math.radians(38)
marks=[]; acc=0.0
for (x0,y0),(x1,y1) in zip(samples,samples[1:]):
    seg=math.hypot(x1-x0,y1-y0)
    if seg==0: continue
    tx,ty=(x1-x0)/seg,(y1-y0)/seg
    acc+=seg
    if acc>=step:
        acc-=step
        nx,ny=-ty,tx
        # diagonal: normal rotated toward tangent
        dx=nx*math.cos(ang)+tx*math.sin(ang); dy=ny*math.cos(ang)+ty*math.sin(ang)
        L=W*0.75
        marks.append(f"M{x1-dx*L:.2f} {y1-dy*L:.2f}L{x1+dx*L:.2f} {y1+dy*L:.2f}")
json.dump({"rope":d,"twist":"".join(marks),"start":P[0],"end":P[-1]},open(sys.argv[2],"w"))
