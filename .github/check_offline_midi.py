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
old=Path(os.environ["QA_MIDI_SOURCE"]).resolve()
relative=Path("midi/models/muscriptor-small")
files={"config.json":"3008fc481e4a1cd978e337eb3759260c270892204db5039235ac939e1f42aeb2",
       "model.safetensors":"bbd482c786b895cf7d8f44185073d951adae2ebb8a66f82ca84cd1f84569549c"}
def digest(path):
    with path.open("rb") as stream: return hashlib.file_digest(stream,"sha256").hexdigest()
for name,sha in files.items():
    source=old/relative/name
    assert digest(source)==sha
broken=root/"wrong"
(broken/relative).mkdir(parents=True)
(broken/relative/"config.json").write_bytes(b"x"*124)
shared=root/"registry"
shared.mkdir()
# Nothing registers this library as another Studio. Only the folder picker knows it.
(broken/relative/"config.json").rename(old/"config.json")
expected=["muscriptor-small-config","muscriptor-small-model","llama-cpu"]
llama_archive=old/("llama-b12556-bin-win-cpu-x64.zip" if os.name=="nt" else "llama-cpu.tar.gz")
llama_sha="98882a3ca45688ffa585c3adba49213ca90c5b2dbbf1bc2af41ac437a0987d17" if os.name=="nt" else "1a573b6089d29c9048eefe8497dd40cac4115400f223d33f2a71e506bf6d5b18"
assert digest(llama_archive)==llama_sha
if os.name=="nt":
    import io,zipfile
    ort_url="https://github.com/timoncool/onnxruntime/releases/download/v1.32.0/onnxruntime-win-x64-1.32.0.zip"
    with urlopen(ort_url,timeout=120) as response: ort_zip=response.read()
    assert hashlib.sha256(ort_zip).hexdigest()=="aaa1d04fcf86bd31e84b90bc0cbde80e632bd0720dcc5d8ba05c4fe45c09d127"
    with zipfile.ZipFile(io.BytesIO(ort_zip)) as z:
        main, provider="onnxruntime.dll","onnxruntime_providers_shared.dll"
        (old/main).write_bytes(z.read(next(n for n in z.namelist() if n.endswith("/"+main))))
        provider_bytes=z.read(next(n for n in z.namelist() if n.endswith("/"+provider)))
    expected.append("onnxruntime")
    url="https://files.pythonhosted.org/packages/59/df/e7c3a360be4f7b93cee39271b792669baeb3846c58a4df6dfcf187a7ffab/nvidia_cuda_runtime_cu12-12.9.79-py3-none-win_amd64.whl"
    with urlopen(url,timeout=120) as response: wheel=response.read()
    assert hashlib.sha256(wheel).hexdigest()=="8e018af8fa02363876860388bd10ccb89eb9ab8fb0aa749aaf58430a9f7c4891"
    archive=old/url.split("/")[-1]
    archive.write_bytes(wheel)
    expected.append("audiocpp-nvidia_cuda_runtime_cu12")
else:
    archive=old/"audiocpp-server-linux-cpu-x64.tar.gz"
    expected.append("audiocpp-audiocpp-cpu")
archive_sha=digest(archive)
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
    with urlopen(req,timeout=120) as res: return json.load(res)
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
        result=call("/setup/adopt", {"path":str(old)})
        assert sorted(result["adopted"])==sorted(expected),result
        assert [Path(row["file"]).resolve() for row in result["refused"]]==[(old/"config.json").resolve()],result["refused"]
        if os.name=="nt":
            def ort_installed():
                return next(row["installed"] for row in call("/v1/karaoke/status")["assets"] if row["id"]=="onnxruntime")
            assert not ort_installed()
            (old/provider).write_bytes(provider_bytes)
            completed=call("/setup/adopt", {"path":str(old)})
            assert completed["adopted"]==["onnxruntime"] and ort_installed(),completed
            expected.append("onnxruntime")
            print("Both unpacked ONNX Runtime 1.32 DLLs verified; incomplete pair stayed unavailable",flush=True)
        again=call("/setup/adopt", {"path":str(old)})
        assert again["adopted"]==[] and again["verified_optional"]==len(expected),again
        if os.name=="nt":
            installed_dll=studio/"audiocpp/runtime/audiocpp-cuda12/cudart64_12.dll"
            assert digest(installed_dll)=="760c38928bbe5759f7b31ed6692599eb7ec83cedd5702e84c2b72028a89837e1"
        assert digest(archive)==archive_sha
        assert digest(llama_archive)==llama_sha
        assistant=call("/v1/assistant/runtime")
        assert next(row["installed"] for row in assistant["assets"] if row["id"]=="llama-cpu"),assistant
        status=call("/v1/midi")
        installed=next(x["installed"] for x in status["sizes"] if x["id"]=="small")
        assert installed,status
        for name,sha in files.items():
            assert digest(old/relative/name)==sha
            target=studio/relative/name
            if not legacy:
                assert digest(target)==sha and target.samefile(old/relative/name),name
        assert (old/"config.json").read_bytes()==b"x"*124
        print(json.dumps({"adopted":result["adopted"], "duplicate":result["refused"], "second_pass":again["adopted"], "installed":installed, "source_sha256":files}),flush=True)
        config=studio/relative/"config.json"
        config.unlink()
        config.write_bytes(b"X"*len((old/relative/"config.json").read_bytes()))
        repaired=call("/setup/adopt",{"path":str(old)})
        assert repaired["adopted"]==["muscriptor-small-config"] and digest(config)==files["config.json"],repaired
        assert config.samefile(old/relative/"config.json")
        print("Verified repair of a same-size damaged installed model config",flush=True)
        import http.client
        from shutil import copyfile
        archived=root/"assistant only"
        archived.mkdir()
        copyfile(llama_archive,archived/llama_archive.name)
        runtime=studio/"assistant/runtime"
        executable=runtime/"cpu"/("llama-server.exe" if os.name=="nt" else "llama-server")
        assert executable.is_file(),executable
        executable.rename(root/"old executable")
        client=http.client.HTTPConnection("127.0.0.1",port,timeout=60)
        client.request("POST","/setup/adopt",json.dumps({"path":str(archived)}),{"Content-Type":"application/json"})
        mark=runtime/("llama-cpu.zip.json.installing" if os.name=="nt" else "llama-cpu.tar.gz.json.installing")
        limit=time.monotonic()+60
        while not mark.exists():
            assert time.monotonic()<limit,mark
            time.sleep(.002)
        client.close()
        while mark.exists() or not executable.exists():
            assert time.monotonic()<limit,"extractor stayed busy after HTTP disconnect"
            time.sleep(.025)
        for _ in range(100):
            assistant=call("/v1/assistant/runtime")
            progress=assistant.get("active_download") or {}
            if progress.get("done") and not progress.get("error"): break
            time.sleep(.05)
        assert progress.get("done") and not progress.get("error"),assistant
        assert digest(archived/llama_archive.name)==llama_sha
        print("Closed HTTP during real assistant extraction; record and download progress reached done",flush=True)
        if "--verify-files" in sys.argv:
            sys.exit(0)
        pcm=Path(os.environ["QA_PIANO_WAV"]).resolve()
        start=call("/v1/midi/transcribe",dict(path=str(pcm),size="small"))
        assert start["started"],start
        deadline=time.monotonic()+240
        while time.monotonic()<deadline:
            run=call("/v1/midi")["run"]
            if run and run["done"]: break
            time.sleep(.5)
        assert run["done"] and not run["error"],run
        midi=Path(run["file"])
        assert midi.read_bytes().startswith(b"MThd"),midi
        pairs=json.loads(Path(str(midi)+".json").read_text())["notes"]
        assert [n["pitch"] for n in pairs]==[60,64,67,72],pairs[:10]
        assert all(n["instrument"]=="acoustic_piano" for n in pairs)
        assert all(abs(n["start"]-t)<.05 for n,t in zip(pairs,[.26,1.26,2.25,3.25]))
        print(json.dumps(dict(offline_notes=pairs, wav_sha256=digest(pcm),midi_sha256=digest(midi))),flush=True)

        print(json.dumps(dict(legacy=legacy,installed=installed,source_hashes=files,profile=str(root))))
    finally:
        proc.terminate()
        proc.wait(timeout=15)
