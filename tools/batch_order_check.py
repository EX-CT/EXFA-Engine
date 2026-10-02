#!/usr/bin/env python3
# Batch output-order stress check: feeds the bench corpus through `batch` as a pipe in random-sized chunks with
# random pauses (so lone requests hit the inline fast path while earlier results are still being written) and
# checks every response is in input order. usage: batch_order_check.py BIN [dataset] (needs /tmp/jv/all.jsonl + base)
import subprocess,sys,time,random,threading
B=sys.argv[1]; D=sys.argv[2] if len(sys.argv)>2 else '/workspace/exct-eve/data/dataset-3569502.json.gz'
L=open('/tmp/jv/all.jsonl').read().splitlines()
exp=open('/tmp/jv/base_all').read().splitlines()
p=subprocess.Popen([B,'--dataset',D,'batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
order=[]
def feed():
    rnd=random.Random(1)
    for it in range(300):
        k=rnd.randrange(len(L)); n=rnd.choice([1,1,1,5,40])
        idx=[(k+j)%len(L) for j in range(n)]
        order.extend(idx)
        p.stdin.write(('\n'.join(L[i] for i in idx)+'\n').encode()); p.stdin.flush()
        if rnd.random()<0.5: time.sleep(rnd.random()*0.002)
    p.stdin.close()
t=threading.Thread(target=feed); t.start()
out=p.stdout.read().decode().splitlines(); t.join()
bad=sum(1 for a,b in zip(order,out) if exp[a]!=b)
print(B, 'lines',len(out),len(order),'misordered',bad)
