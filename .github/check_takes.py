import hashlib
import json
from pathlib import Path
import time
from urllib.request import Request,urlopen

def check_takes(api,source,legacy):
    def call(route,body=None):
        with urlopen(Request(api+route,data=json.dumps(body).encode() if body is not None else None,
                             headers={"Content-Type":"application/json"}),timeout=30) as r:
            return json.load(r)
    song=call("/v1/library/songs",dict(title="Take test",audio_path=str(source),caption="Take test",lyrics="",
        engine_id="manual",metadata={},source="import"))
    digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    original=digest(source)
    route="/v1/library/songs/"+song["id"]+"/process"
    output=[]
    for peak in (1,3000):
        call(route,{"normalize":{"peak_clip":peak}})
        deadline=time.monotonic()+90
        while True:
            run=call("/v1/processing")["run"]
            if run["preview_ready"]: break
            assert not run["error"] and time.monotonic()<deadline,run
            time.sleep(.1)
        saved=call("/v1/processing/keep",{"label":"Variant "+str(peak)})
        path=Path(saved["audio_path"])
        output.append((saved["id"],str(path),digest(path)))
    assert output[0][0]!=output[1][0]
    assert (output[0][1]==output[1][1])==legacy,output
    assert original==digest(source)
    if not legacy:
        assert all(digest(Path(path))==sha for _,path,sha in output)
    return {"original_sha256":original,"versions":output,"legacy":legacy}
