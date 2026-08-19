import gzip, json
from datetime import datetime
P="/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726/benchmarks/m9-agentic-local-v1/runs-v2/skill-1/stream.jsonl.gz"
ev=[]
for line in gzip.open(P,'rt',errors='replace'):
    line=line.strip()
    if not line: continue
    try: e=json.loads(line)
    except: continue
    if 'timestamp' not in e: continue
    t=datetime.fromisoformat(e['timestamp'].replace('Z','+00:00'))
    kind=e.get('type'); chars=0; btype=None; inp=None
    if kind=='assistant':
        m=e.get('message') or {}
        inp=(m.get('usage') or {}).get('input_tokens')
        for b in m.get('content') or []:
            btype=b.get('type')
            if btype=='thinking': chars=len(b.get('thinking') or b.get('text') or '')
            elif btype=='text':   chars=len(b.get('text') or '')
    ev.append((t,kind,btype,chars,inp))
ev.sort(key=lambda x:x[0])

PREFILL=311.0   # operator spec, 32k in 102.9s
rows=[]
for i in range(1,len(ev)):
    dt=(ev[i][0]-ev[i-1][0]).total_seconds()
    _,kind,btype,chars,inp=ev[i]
    if kind=='assistant' and chars>0 and inp:
        tok=chars/4
        pre=inp/PREFILL                      # cache MISSING: full context recomputed
        dec=max(dt-pre,0.001)
        rows.append((dt,btype,chars,tok,inp,pre,dec,tok/dec))

print(f"{'gap_s':>8} {'block':>8} {'~tok':>6} {'ctx_tok':>8} {'prefill_s':>9} {'decode_s':>8} {'tok/s':>7}")
for r in sorted(rows,reverse=True)[:8]:
    print(f"{r[0]:8.1f} {r[1]:>8} {r[3]:6.0f} {r[4]:8d} {r[5]:9.1f} {r[6]:8.1f} {r[7]:7.1f}")

big=[r for r in rows if r[2]>10000]
tt=sum(r[0] for r in big); tk=sum(r[3] for r in big); tp=sum(r[5] for r in big); td=sum(r[6] for r in big)
print()
print(f"3 largest thinking blocks: gap {tt:.0f}s = prefill {tp:.0f}s + decode {td:.0f}s for ~{tk:.0f} tok")
print(f"  OLD rate (gap only, no prefill split): {tk/tt:.1f} tok/s")
print(f"  CORRECTED rate                       : {tk/td:.1f} tok/s")
print(f"  operator's deep-reasoning rate       : 16.5 tok/s")
allt=sum(r[0] for r in rows); allp=sum(r[5] for r in rows); alld=sum(r[6] for r in rows); allk=sum(r[3] for r in rows)
print()
print(f"all generating gaps: {allt:.0f}s = prefill {allp:.0f}s ({100*allp/allt:.0f}%) + decode {alld:.0f}s ({100*alld/allt:.0f}%)")
print(f"  corrected overall rate: {allk/alld:.1f} tok/s over ~{allk:.0f} tokens")
