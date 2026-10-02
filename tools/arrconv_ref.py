import json,subprocess,random
D="/workspace/exct-eve/data/dataset-3569502.json.gz"; A="/tmp/refhead/target/release/eve-dogma"; J="/workspace/exct-eve/lab-j/variant-j/build/eve-dogma-j"  # struct-as-array parity check vs eve-dogma-rs
S={'fit':[('schema_version',None),('ship','ship'),('character','char'),('modules',['mod']),('drones',['drone']),('fighters',['fighter']),('implants',None),('boosters',['booster']),('cargo',['cargo']),('fleet','fleet'),('projected',['proj']),('environment','env'),('damage_pattern','res'),('target_profile','tp'),('overrides',['ovr']),('options','opt')],
'ship':[('type_id',None),('mode_type_id',None)],'char':[('skills','skills'),('security_status',None)],'skills':[('default_level',None),('levels',None)],
'mod':[('type_id',None),('slot',None),('state',None),('charge_type_id',None),('mutation','mut'),('spool','spool')],'mut':[('base_type_id',None),('mutaplasmid_type_id',None),('attributes',None)],
'spool':[('type',None),('amount',None)],'drone':[('type_id',None),('quantity',None),('active',None),('mutation','mut')],'fighter':[('type_id',None),('quantity',None),('active',None),('abilities',None)],
'booster':[('type_id',None),('side_effects',None)],'cargo':[('type_id',None),('quantity',None)],'fleet':[('buffs',['buff']),('booster_fits',['fit'])],'buff':[('buff_id',None),('value',None)],
'proj':[('kind',None),('module','mod'),('drone','drone'),('fit','fit'),('fighter','fighter'),('amount',None),('distance_m',None)],'env':[('effect_type_ids',None),('system_security',None)],
'res':[('em',None),('thermal',None),('kinetic',None),('explosive',None)],'tp':[('em',None),('thermal',None),('kinetic',None),('explosive',None),('signature_radius',None),('max_velocity',None),('radius',None)],
'ovr':[('type_id',None),('attribute_id',None),('value',None)],'opt':[('nos_no_target_cap',None),('factor_reload',None),('default_spool','spool'),('rah',None),('include_attributes',None),('sources',None),('validate',None),('cap_sim','cs')],'cs':[('reload',None),('stagger',None),('max_time_s',None)]}
def conv(v,t,p):
    if t is None or v is None: return v
    if isinstance(t,list):
        return [conv(x,t[0],p) for x in v] if isinstance(v,list) else v
    if not isinstance(v,dict): return v
    fields=S[t]; d={k:conv(v[k],ft,p) for k,ft in fields if k in v}
    if random.random()<p and all(k in [f for f,_ in fields] for k in v):
        names=[f for f,_ in fields]; last=max([names.index(k) for k in d]+[-1])
        # fill gaps with defaults is not possible generically -> only convert if keys form a prefix
        if all(names[i] in d for i in range(last+1)): return [d[names[i]] for i in range(last+1)]
    return d
random.seed(7)
reqs=[json.loads(l) for l in open('/tmp/jv/all.jsonl')]
L=[json.dumps(conv(r,'fit',p)) for r in reqs for p in (0.5,1.0)]
inp='\n'.join(L)+'\n'
a=subprocess.run([A,'--dataset',D,'batch'],input=inp.encode(),capture_output=True).stdout.decode().replace('eve-dogma-rs 0.1.0','E').splitlines()
j=subprocess.run([J,'--dataset',D,'batch'],input=inp.encode(),capture_output=True).stdout.decode().replace('eve-dogma-j 0.1.0','E').splitlines()
bad=[k for k in range(len(a)) if a[k]!=j[k] and not (a[k].startswith('{"error"') and j[k].startswith('{"error"'))]
print('converted',len(L),'arrays-in-input',sum('[[' in x or '":[5' in x for x in L),'ok A',sum(not x.startswith('{"error"') for x in a),'diff',len(bad))
for k in bad[:2]: print(L[k][:200]); print(a[k][:200]); print(j[k][:200])
