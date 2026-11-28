import pathlib
exec((pathlib.Path(__file__).with_name('triangulation_calc.py')).read_text().split('print("== Q1')[0])
src=(pathlib.Path(__file__).with_name('triangulation_calc.py')).read_text()
# reuse functions
ns={}
exec(src.replace('print(','(lambda *a,**k:None)('),ns)
sigma_alt=ns['sigma_alt'];sigma_alt_arc=ns['sigma_alt_arc'];ecef=ns['ecef'];G=ns['G'];sites=ns['sites'];dist=ns['dist'];offset=ns['offset']
print("refraction Bennett (arcmin / mrad), masse d'air Kasten-Young")
for e in (1,2,3,5,10,20,30):
  r=1/np.tan(np.radians(e+7.31/(e+4.4)))
  X=1/(np.sin(np.radians(e))+0.50572*(e+6.07995)**-1.6364)
  print(f"e={e}: R={r:.1f}' = {r/60*np.pi/180*1e3:.2f} mrad ; airmass {X:.1f} ; extinction 0.25 mag/airmass -> {0.25*X:.1f} mag")
print("Kilpisjarvi-Sodankyla", dist((69.01,20.79),(67.42,26.39)), "Abisko-Kilpis", dist((68.36,18.82),(69.01,20.79)))
print("SAR/STEVE quasi zenithal, sigma 1 mrad")
for h in (250,400):
  for L in (45.2,46.5,48):
    P=ecef(L,5.72,h)
    r1=sigma_alt_arc([G,sites['Chambery']],P,1e-3)[0]
    r2=sigma_alt_arc([G,sites['Chambery'],sites['Lyon'],sites['Valence'],sites['Geneve']],P,1e-3)[0]
    r3=sigma_alt_arc([G,sites['Dijon']],P,1e-3)[0]
    rp=sigma_alt([G,sites['Lyon']],P,1e-3)[0]
    print(f"h={h} L={L}: arc G+Chambery {r1:.1f} | arc local5 {r2:.1f} | arc G+Dijon {r3:.1f} | point G+Lyon {rp:.1f}")
print("synchro: dt max pour deplacement < 0.5 mrad a distance oblique")
for s in (550,800,1100):
  for v in (0.3,1,3,5):
    print(f"oblique {s} km v={v} km/s: dt_max {0.5e-3*s/v*1000:.0f} ms ; flou pose 2 s = {2*v/s*1e3:.1f} mrad = {2*v/s/0.574e-3:.1f} px(2.7mm)")
