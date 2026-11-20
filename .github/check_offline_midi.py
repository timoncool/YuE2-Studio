import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from urllib.request import Request,urlopen

legacy="--legacy" in sys.argv
exe=Path(sys.argv[1]).resolve()
root=Path(tempfile.mkdtemp(dir=os.environ.get("RUNNER_TEMP") or "F:/AI/hf-work/music-round-2026-11-20"))
old=Path(sys.argv[2]).resolve() if legacy else root/"MiniMax"/"data"
relative=Path("midi/models/muscriptor-small")
files={"config.json":"3008fc481e4a1cd978e337eb3759260c270892204db5039235ac939e1f42aeb2",
       "model.safetensors":"bbd482c786b895cf7d8f44185073d951adae2ebb8a66f82ca84cd1f84569549c"}
def digest(path):
    with path.open("rb") as stream: return hashlib.file_digest(stream,"sha256").hexdigest()
for name,sha in files.items():
    source=old/relative/name
    if not legacy:
        source.parent.mkdir(parents=True,exist_ok=True)
        subprocess.run(["curl","-fsSL","--retry","3","https://huggingface.co/cocktailpeanut/muscriptor-small/resolve/31a8f75d6a8b5383fd71ad1371dc1620389ab722/"+name,"-o",str(source)],check=True)
    assert digest(source)==sha
broken=root/"wrong"
(broken/relative).mkdir(parents=True)
(broken/relative/"config.json").write_bytes(b"x"*124)
shared=root/"registry"
shared.mkdir()
(shared/"studios.json").write_text(json.dumps(dict(studios=[dict(app=app,data_dir=str(d),seen=t,version="4.0.3") for app,d,t in [("minimax",old,1),("ace",broken,2)]])))
studio=root/"YuE2"
studio.mkdir()
def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1",0))
        return s.getsockname()[1]
port=free_port()
proxy="http://127.0.0.1:"+str(free_port())
env=dict(os.environ,YUE_STUDIO_DATA_ROOT=str(studio),YUE_STUDIO_PORT=str(port),YUE_ENGINE_PORT=str(free_port()),YUE_MODELS_ROOT=str(studio/"models/yue2-cpp"),YUE_ENGINE_BIN=str(studio/"missing.exe"),
         YUE_STUDIO_SETTINGS_PATH=str(studio/"studio-settings.json"),TIMONCOOL_SHARED_DIR=str(shared),STUDIO_TELEMETRY="0",HTTP_PROXY=proxy,HTTPS_PROXY=proxy,ALL_PROXY=proxy,NO_PROXY="127.0.0.1,localhost")
def call(endpoint,body=None):
    req=Request(f"http://127.0.0.1:{port}"+endpoint,data=json.dumps(body).encode() if body else None,headers={"Content-Type":"application/json"})
    with urlopen(req,timeout=5) as res: return json.load(res)
log=root/"service.log"
with log.open("wb") as stream:
    proc=subprocess.Popen([str(exe)],cwd=studio,env=env,stdout=stream,stderr=stream)
    try:
        for _ in range(150):
            if proc.poll() is not None: raise RuntimeError(log.read_text(errors="replace")[-2000:])
            try:
                call("/health")
                break
            except OSError:time.sleep(.2)
        assert call("/v1/midi/install",dict(size="small"))["started"]
        deadline=time.monotonic()+(8 if legacy else 60)
        while time.monotonic()<deadline:
            status=call("/v1/midi")
            installed=next(x["installed"] for x in status["sizes"] if x["id"]=="small")
            if installed: break
            time.sleep(.2)
        assert installed!=legacy,(status,log.read_text(errors="replace")[-1500:])
        for name,sha in files.items():
            assert digest(old/relative/name)==sha
            target=studio/relative/name
            if not legacy:
                assert digest(target)==sha and target.samefile(old/relative/name),name
        assert (broken/relative/"config.json").read_bytes()==b"x"*124
        print(json.dumps(dict(legacy=legacy,installed=installed,source_hashes=files,profile=str(root))))
    finally:
        proc.terminate()
        proc.wait(timeout=15)
