#!/usr/bin/env python3
# Mutation fuzzer: random edits of the bench corpus (/tmp/all.jsonl) fed to J and the reference batch;
# reports crashes and outcome mismatches (success vs error, error codes). Usage: fuzz_ref.py SEED
import json,random,subprocess,sys
random.seed(int(sys.argv[1]) if len(sys.argv)>1 else 1)
reqs=[json.loads(l) for l in open('/tmp/all.jsonl')]
vals=[None,True,False,0,-1,1,2.5,1e308,-1e308,"x","",[],{},[1,2],{"a":1},4294967296,587,2048,-0.0,1e-300]
def mutate(o,depth=0):
    if isinstance(o,dict):
        o=dict(o)
        if o and random.random()<0.3:
            k=random.choice(list(o)); 
            r=random.random()
            if r<0.3: del o[k]
            elif r<0.7: o[k]=random.choice(vals)
            else: o[k]=mutate(o[k],depth+1)
        for k in list(o):
            if random.random()<0.2: o[k]=mutate(o[k],depth+1)
        return o
    if isinstance(o,list):
        o=list(o)
        if o and random.random()<0.3:
            i=random.randrange(len(o)); r=random.random()
            if r<0.3: del o[i]
            elif r<0.6: o[i]=random.choice(vals)
            else: o.append(o[i])
        return [mutate(x,depth+1) if random.random()<0.2 else x for x in o]
    if isinstance(o,(int,float)) and not isinstance(o,bool) and random.random()<0.3: return random.choice(vals)
    return o
L=[]
for i in range(3000):
    r=mutate(random.choice(reqs))
    s=json.dumps(r)
    if random.random()<0.03: s=s[:random.randrange(len(s))]
    L.append(s.replace('\n',' '))
inp='\n'.join(L)+'\n'
J='/workspace/exct-eve/lab-j/variant-j/build/eve-dogma-j'; D='/workspace/exct-eve/data/dataset-3569502.json.gz'
p=subprocess.run([J,'--dataset',D,'batch'],input=inp.encode(),capture_output=True,timeout=600)
out=p.stdout.decode().splitlines()
print('rc',p.returncode,'in',len(L),'out',len(out), p.stderr[-300:])
A='/tmp/refhead/target/release/eve-dogma'
pa=subprocess.run([A,'--dataset',D,'batch'],input=inp.encode(),capture_output=True,timeout=900)
ao=pa.stdout.decode().replace('eve-dogma-rs 0.1.0','E').splitlines(); jo=[x.replace('eve-dogma-j 0.1.0','E') for x in out]
print('ref rc',pa.returncode,len(ao), pa.stderr.decode()[-300:])
same=diff=errcode=0; ex=[]
for k,(a,j) in enumerate(zip(ao,jo)):
    if a==j: same+=1; continue
    ea=a.startswith('{"error"'); ej=j.startswith('{"error"')
    if ea and ej:
        if json.loads(a)['error']['code']==json.loads(j)['error']['code']: errcode+=1; continue
    diff+=1; ex.append(k)
print('same',same,'err-code-match',errcode,'diff',diff)
for k in ex[:3]: print(L[k][:300]); print('A',ao[k][:300]); print('J',jo[k][:300])
import re,collections
cnt=collections.Counter()
for k in ex:
    a=ao[k]
    if a.startswith('{"error"'):
        m=re.search(r'column (\d+)',a); msg=json.loads(a)['error']['message']
        if m:
            c=int(m.group(1)); ctx=L[k][max(0,c-60):c]
            key=re.findall(r'"(\w+)":',ctx)
            cnt['A-err '+re.sub(r' at line.*','',msg)[:40]+' @'+(key[-1] if key else '?')]+=1
        else: cnt['A-err '+msg[:50]]+=1
    else:
        cnt['J-err '+json.loads(jo[k])['error']['message'][:60]]+=1
for x,v in cnt.most_common(40): print(v,x)
