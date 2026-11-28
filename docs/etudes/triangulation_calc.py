# Calculs de l annexe D de docs/AUDIT_AMELIORATIONS.md (triangulation). Lancer : python3 triangulation_calc.py
import numpy as np
R=6371.0
d2r=np.pi/180
print("== Q1 geometrie")
for h in (110,200,300):
  for L in (50,52,55):
    th=(L-45.2)*d2r
    e=np.degrees(np.arctan2(np.cos(th)-R/(R+h),np.sin(th)))
    s=np.sqrt(R**2+(R+h)**2-2*R*(R+h)*np.cos(th))
    print(f"h={h} L={L} sol={R*th:.0f} km elev={e:.1f} deg oblique={s:.0f} km")
# horizon limit: latitude max visible at elev 0 and at 5,10 deg
for h in (110,200,300):
  for emin in (0,5,10):
    # central angle for elevation emin: th = acos(R cos e/(R+h)) - e
    e=emin*d2r
    th=np.arccos(R*np.cos(e)/(R+h))-e
    print(f"h={h} elev_min={emin}: portee {R*th:.0f} km -> lat {45.2+np.degrees(th):.1f}N")
print("== Q2 pixel")
p=1.55e-3
for f in (2.7,1.6,1.4,1.8):
  a=p/f
  print(f"f={f}: {a*1e3:.3f} mrad = {np.degrees(a)*60:.2f} arcmin = {np.degrees(a):.4f} deg ; bin2 {2*np.degrees(a)*60:.2f} arcmin; rayon 90deg {f*np.pi/2:.2f} mm")
print("demi-dim capteur mm", 4056*p/2, 3040*p/2, np.hypot(4056,3040)*p/2)
print("champ 2.7 sur petit axe (deg total)", 2*np.degrees(3040*p/2/2.7), "grand axe",2*np.degrees(4056*p/2/2.7))

def ecef(lat,lon,h):
  lat,lon=lat*d2r,lon*d2r
  r=R+h
  return np.array([r*np.cos(lat)*np.cos(lon),r*np.cos(lat)*np.sin(lon),r*np.sin(lat)])
def enu_basis(lat,lon):
  lat,lon=lat*d2r,lon*d2r
  E=np.array([-np.sin(lon),np.cos(lon),0])
  N=np.array([-np.sin(lat)*np.cos(lon),-np.sin(lat)*np.sin(lon),np.cos(lat)])
  U=np.array([np.cos(lat)*np.cos(lon),np.cos(lat)*np.sin(lon),np.sin(lat)])
  return E,N,U
def sigma_alt(stations,P,sig):
  # each station gives 2 angular constraints perpendicular to line of sight with std sig*range
  A=[];W=[]
  for (la,lo) in stations:
    S=ecef(la,lo,0)
    v=P-S; rng=np.linalg.norm(v); u=v/rng
    # check above horizon
    E,N,U=enu_basis(la,lo)
    el=np.degrees(np.arcsin(u@U))
    if el<0: return None
    # two perpendicular directions
    a=np.cross(u,U); a/=np.linalg.norm(a)
    b=np.cross(u,a)
    for w in (a,b):
      A.append(w); W.append(1/(sig*rng)**2)
  A=np.array(A);W=np.diag(W)
  C=np.linalg.inv(A.T@W@A)
  Up=P/np.linalg.norm(P)
  return np.sqrt(Up@C@Up), np.sqrt(np.trace(C))
G=(45.19,5.72)
def offset(lat,lon,dN,dE):
  return (lat+dN/R/d2r, lon+dE/(R*np.cos(lat*d2r))/d2r)
print("== Q3 erreur altitude pour sigma=1 mrad (~3.4 arcmin) ; lineaire en sigma")
sig=1e-3
for h in (110,200,300):
  for L in (50,52,55):
    P=ecef(L,5.72,h)
    out=[]
    for B in (20,50,100,200,500):
      # NS: second station B km north of Grenoble
      ns=sigma_alt([G,offset(*G,B,0)],P,sig)
      ew=sigma_alt([offset(*G,0,-B/2),offset(*G,0,B/2)],P,sig)
      out.append(f"B={B}: NS {ns[0]:.1f} / EW {ew[0]:.1f}" if ns and ew else f"B={B}: n/a")
    print(f"h={h} L={L}: "+" | ".join(out))
# single station altitude error if height assumed: not applicable; compute horizontal pos error per km alt
print("== Erreur de position horizontale (une station, altitude supposee) : d(sol)/d(h)")
for h in (110,200,300):
  for L in (50,52,55):
    th=(L-45.2)*d2r
    e=np.arctan2(np.cos(th)-R/(R+h),np.sin(th))
    # find th for h+10 at same elev
    f=lambda t: np.arctan2(np.cos(t)-R/(R+h+10),np.sin(t))-e
    lo,hi=th,th*2
    for _ in range(100):
      m=(lo+hi)/2
      if f(m)>0: lo=m
      else: hi=m
    t2=m
    print(f"h={h} L={L}: +10 km altitude => +{R*(t2-th):.0f} km au sol")

def sigma_alt_arc(stations,P,sig):
  # arc allonge est-ouest : position le long de l'arc inconnue ; seule la composante perpendiculaire a l'arc projete est mesurable
  E,N,U=enu_basis(*[np.degrees(x) for x in (np.arcsin(P[2]/np.linalg.norm(P)),np.arctan2(P[1],P[0]))])
  T=E
  A=[];W=[]
  for (la,lo) in stations:
    S=ecef(la,lo,0); v=P-S; rng=np.linalg.norm(v); u=v/rng
    tp=T-(T@u)*u; tp/=np.linalg.norm(tp)
    w=np.cross(u,tp)  # direction perpendiculaire a l'arc dans le plan image
    A.append([w@N,w@U]); W.append(1/(sig*rng)**2)
  A=np.array(A);W=np.diag(W)
  C=np.linalg.inv(A.T@W@A)
  return np.sqrt(C[1,1]), np.sqrt(C[0,0])
print("== Q3b ARC est-ouest, sigma=1 mrad")
for h in (110,200,300):
  for L in (50,52,55):
    P=ecef(L,5.72,h)
    out=[]
    for B in (20,50,100,200,500):
      ns=sigma_alt_arc([G,offset(*G,B,0)],P,sig)
      try:
        ew=sigma_alt_arc([offset(*G,0,-B/2),offset(*G,0,B/2)],P,sig)[0]
        ew=f"{ew:.0f}" if ew<1e5 else "inf"
      except np.linalg.LinAlgError: ew="inf"
      out.append(f"B={B}: NS {ns[0]:.1f} / EW {ew}")
    print(f"h={h} L={L}: "+" | ".join(out))
print("== Configurations reelles")
sites={"Grenoble":(45.19,5.72),"Lyon":(45.76,4.84),"Dijon":(47.32,5.04),"Nancy":(48.69,6.18),"Lille":(50.63,3.06),"Chambery":(45.57,5.92),"Valence":(44.93,4.89),"Geneve":(46.20,6.14),"Besancon":(47.24,6.02),"Paris":(48.86,2.35),"Strasbourg":(48.58,7.75)}
def dist(a,b):
  a=np.array(a)*d2r;b=np.array(b)*d2r
  return R*np.arccos(np.sin(a[0])*np.sin(b[0])+np.cos(a[0])*np.cos(b[0])*np.cos(a[1]-b[1]))
for s in ("Chambery","Lyon","Geneve","Dijon","Besancon","Nancy","Strasbourg","Paris","Lille"):
  print(s, f"{dist(G,sites[s]):.0f} km")
for sig_name,sig in (("0.5 mrad",0.5e-3),("1 mrad",1e-3),("3 mrad",3e-3),("5 mrad",5e-3)):
  for h,L in ((110,52),(110,55),(300,50),(300,52)):
    P=ecef(L,5.72,h)
    row=[]
    for s in ("Chambery","Lyon","Dijon","Nancy","Lille"):
      r=sigma_alt([G,sites[s]],P,sig)
      row.append(f"{s} {r[0]:.1f}" if r else f"{s} n/a")
    r=sigma_alt([G,sites['Chambery'],sites['Lyon'],sites['Valence'],sites['Geneve']],P,sig)
    row.append(f"reseau local5 {r[0]:.1f}")
    r=sigma_alt([G,sites['Lyon'],sites['Dijon'],sites['Nancy']],P,sig)
    row.append(f"G+Lyon+Dijon+Nancy {r[0]:.1f}")
    ra=[f"{s} {sigma_alt_arc([G,sites[s]],P,sig)[0]:.1f}" for s in ("Chambery","Lyon","Dijon","Nancy","Lille")]
    row.append("ARC: "+", ".join(ra))
    row.append("ARC local5 %.1f"%sigma_alt_arc([G,sites['Chambery'],sites['Lyon'],sites['Valence'],sites['Geneve']],P,sig)[0])
    print(f"sig={sig_name} h={h} L={L}: "+" | ".join(row))
print("== Q4 temporel")
for v in (0.5,1,2,5,10):
  for dt in (0.01,0.1,1):
    pass
  print(f"v={v} km/s: deplacement en 0.1 s {v*0.1:.2f} km, 1 s {v:.0f} km, pose 5 s {5*v:.0f} km")
# angular motion at slant 800 km
for s_km in (700,1000,1500):
  print(f"oblique {s_km} km: 1 pixel 2.7mm = {s_km*0.574e-3:.2f} km ; bin2 {2*s_km*0.574e-3:.2f} km; 1.6mm {s_km*0.969e-3:.2f} km")
