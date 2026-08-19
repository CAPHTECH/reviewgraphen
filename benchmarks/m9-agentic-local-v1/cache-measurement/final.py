import gzip, json
P="/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726/benchmarks/m9-agentic-local-v1/runs-v2/skill-1/stream.jsonl.gz"
ins=[]; think=0; text=0
for line in gzip.open(P,'rt',errors='replace'):
    line=line.strip()
    if not line: continue
    try: e=json.loads(line)
    except: continue
    if e.get('type')!='assistant': continue
    m=e.get('message') or {}
    u=m.get('usage') or {}
    if u.get('input_tokens') is not None: ins.append(u['input_tokens'])
    for b in m.get('content') or []:
        if b.get('type')=='thinking': think+=len(b.get('thinking') or b.get('text') or '')
        elif b.get('type')=='text':  text+=len(b.get('text') or '')

calls=[]
for v in ins:
    if not calls or calls[-1]!=v: calls.append(v)
SPAN=5275.5; OUT_REPORTED=20264; OUT_EST=(think+text)/4
ctx=sum(calls)
print(f"API calls: {len(calls)}   summed context: {ctx:,} tokens (== authoritative input_tokens)")
print(f"observed span: {SPAN:.0f}s   reported output: {OUT_REPORTED:,} tok   char-estimated: {OUT_EST:,.0f} tok")
print()
for rate,label in ((311,'operator spec 32k/102.9s'),(267,'my novel-prefix measurement')):
    pre=ctx/rate; dec=SPAN-pre
    print(f"prefill at {rate} tok/s ({label}): {pre:6.0f}s  -> decode {dec:6.0f}s")
    if dec>0:
        print(f"    decode rate: {OUT_REPORTED/dec:5.1f} tok/s (reported)  |  {OUT_EST/dec:5.1f} tok/s (char-estimated)")
print()
print("operator's stated deep-reasoning decode rate: 16.5 tok/s")
print()
pre=ctx/311; dec=SPAN-pre
print(f"BUDGET (prefill at 311 tok/s): prefill {pre:.0f}s = {100*pre/SPAN:.0f}% | decode {dec:.0f}s = {100*dec/SPAN:.0f}%")
print(f"cache-working counterfactual: delta-only prefill {(calls[-1]-calls[0])/311:.0f}s + decode {OUT_REPORTED/16.5:.0f}s = {(calls[-1]-calls[0])/311 + OUT_REPORTED/16.5:.0f}s")
print(f"  i.e. the same work in ~{100*((calls[-1]-calls[0])/311 + OUT_REPORTED/16.5)/SPAN:.0f}% of the wall clock")
