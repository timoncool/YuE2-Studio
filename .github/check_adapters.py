import hashlib
import struct
import requests
def check_adapter_import(api,profile,proof,legacy):
    records=[]
    for value in (1.,2.):
        header=b'{"lora_A.weight":{"dtype":"F32","shape":[1,1],"data_offsets":[0,4]}}'
        header+=b" "*(-len(header)%8)
        data=struct.pack("<Q",len(header))+header+struct.pack("<f",value)
        result=requests.post(api+"/v1/adapters/import",data={"name":"Same voice"},
                             files={"files":("voice.safetensors",data)},timeout=15)
        result.raise_for_status()
        records.append((result.json()["id"],hashlib.sha256(data).hexdigest()))
    saved=[]
    for adapter_id,_ in records:
        path=profile/"adapters"/adapter_id/"voice.safetensors"
        saved.append(hashlib.sha256(path.read_bytes()).hexdigest())
    collision=records[0][0]==records[1][0]
    assert collision==legacy,(records,saved)
    if legacy: assert saved[0]==records[1][1]!=records[0][1]
    else: assert saved==[sha for _,sha in records]
    import json
    out=dict(ids=[key for key,_ in records],uploaded=[sha for _,sha in records],stored=saved)
    (proof/("yue2-same-name-loras-"+("before" if legacy else "after")+".json")).write_text(json.dumps(out,indent=2))
    print(out)
