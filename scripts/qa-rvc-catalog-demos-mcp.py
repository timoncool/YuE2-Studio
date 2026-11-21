import array
import io
import math
import struct
import wave
import hashlib
import json
import os
import socket
import subprocess
import tempfile
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import ProxyHandler, Request, build_opener

INDEXED = "rvc-50ea3d733073550ef6f7"
BARE = "rvc-88ad512ce774a929972e"
DEMO = "rvc-3d114ade3fdf27050c8a"
HTTP = build_opener(ProxyHandler({}))


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def make_fixture(root):
    speech = root / "spoken-vocals.wav"
    subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", """
Add-Type -AssemblyName System.Speech
$s = [System.Speech.Synthesis.SpeechSynthesizer]::new()
$f = [System.Speech.AudioFormat.SpeechAudioFormatInfo]::new(44100, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$s.SetOutputToWaveFile($env:RVC_QA_SPEECH, $f)
$s.Speak("The morning wind is gentle. We can sing and hear the river flowing through the valley.")
$s.Dispose()
"""], env=dict(os.environ, RVC_QA_SPEECH=str(speech)), check=True)
    with wave.open(str(speech), "rb") as reader:
        assert reader.getnchannels() == 1 and reader.getsampwidth() == 2
        rate = reader.getframerate()
        voice = array.array("h", reader.readframes(reader.getnframes()))
    left = [int(1500 * math.sin(2 * math.pi * 330 * i / rate)) for i in range(len(voice))]
    right = [int(1700 * math.sin(2 * math.pi * 440 * i / rate)) for i in range(len(voice))]
    backing = array.array("h", (value for frame in zip(left, right) for value in frame))
    mixed = array.array("h", (max(-32768, min(32767, sample + tone))
                             for sample, l, r in zip(voice, left, right) for tone in (l, r)))
    def write(name, data):
        path = root / name
        with wave.open(str(path), "wb") as writer:
            writer.setparams((2, 2, rate, 0, "NONE", "not compressed"))
            writer.writeframes(data.tobytes())
        return path
    return speech, write("instrumental.wav", backing), write("original.wav", mixed), rate, len(voice)


def wave_shape(body):
    assert body[:4] == b"RIFF" and body[8:12] == b"WAVE"
    offset = 12
    while offset + 8 <= len(body):
        kind, length = struct.unpack_from("<4sI", body, offset)
        start = offset + 8
        if kind == b"fmt ":
            code, channels, rate, _, align, bits = struct.unpack_from("<HHIIHH", body, start)
        if kind == b"data":
            return code, channels, rate, bits, length // align
        offset = start + length + (length & 1)
    raise AssertionError("WAV has no audio")



def run():
    binary=Path("target/debug/music-server.exe").resolve()
    studio, engine=port(), port()
    base=f"http://127.0.0.1:{studio}"
    with tempfile.TemporaryDirectory(prefix="yue-rvc-catalog-demos-", ignore_cleanup_errors=True) as folder:
        root=Path(folder)
        (root/"data").mkdir()
        env=dict(os.environ,YUE_STUDIO_PORT=str(studio),YUE_ENGINE_PORT=str(engine),
            YUE_STUDIO_DATA_ROOT=str(root/"data"),YUE_ENGINE_BIN=str(root/"absent.exe"),
            TIMONCOOL_SHARED_DIR=str(root/"registry"),DO_NOT_TRACK="1",STUDIO_HUB_URL=base+"/fixture-hub")
        count=0
        def mcp(name, args, timeout=35):
            nonlocal count
            count+=1
            payload=dict(jsonrpc="2.0",id=count,method="tools/call",params=dict(name=name,arguments=args))
            with HTTP.open(Request(base+"/mcp",json.dumps(payload).encode(),
                {"Content-Type":"application/json","Accept":"application/json, text/event-stream"}),timeout=timeout) as response:
                answer=json.load(response)
            assert "error" not in answer and not answer["result"].get("isError"),answer
            result=answer["result"]
            return result["structuredContent"] if "structuredContent" in result else json.loads(result["content"][0]["text"])
        def read(url):
            with HTTP.open(base+url,timeout=45) as r:
                return r.headers.get_content_type(),r.read()
        with (root/"studio.log").open("wb") as log, subprocess.Popen([str(binary)],env=env,stdout=log,stderr=subprocess.STDOUT) as proc:
            try:
                for _ in range(120):
                    assert proc.poll() is None,proc.returncode
                    try:
                        read("/health")
                        break
                    except URLError:
                        time.sleep(.25)
                else:
                    raise TimeoutError("Studio")
                first=mcp("rvc_catalog_list",{"has_sample":True})
                assert first["total"]==37 and len(first["voices"])==20 and first["next_offset"]==20,first
                rest=mcp("rvc_catalog_list",{"has_sample":True,"offset":20})
                assert rest["total"]==37 and len(rest["voices"])==17 and rest["next_offset"] is None,rest
                assert len({voice["id"] for voice in first["voices"]+rest["voices"]})==37
                example=next(v for v in first["voices"]+rest["voices"] if v["id"]==DEMO)
                assert example["installed_id"] is None
                media_type, contents=read(example["sample_audio_url"])
                assert media_type=="audio/mpeg" and len(contents)==example["sample"]["size"]
                assert hashlib.sha256(contents).hexdigest()==example["sample"]["sha256"]
                assert mcp("rvc_voices_list",{})==[]
                try:
                    read("/v1/rvc/catalog/sample/"+INDEXED)
                except HTTPError as error:
                    assert error.code==404
                else:
                    raise AssertionError("unpublished Radio Tapok sample unexpectedly loaded")
                indexed=mcp("rvc_catalog_list",dict(query="Radio Tapok",vocal_style="high",rvc_version="v2",sample_rate_hz=40000,has_index=True,has_sample=False))
                assert indexed["total"]==1 and indexed["voices"][0]["id"]==INDEXED,indexed
                assert indexed["voices"][0]["sample_audio_url"] is None
                no_index=mcp("rvc_catalog_list",dict(query="youzhanv2-xi",rvc_version="v2",sample_rate_hz=48000,has_index=False,has_sample=False))
                assert no_index["total"]==1 and no_index["voices"][0]["id"]==BARE,no_index
                assert mcp("rvc_apply_install",{},timeout=900)["backends"]["audio.cpp"]["ready"]
                speech, instrumental, original, rate, frames=make_fixture(root)
                added=mcp("library_import_audio",dict(path=str(original),title="RVC catalog fixture"))
                song_id=added.get("song",added)["id"]
                original_bytes=read("/v1/library/media/"+song_id)[1]
                media=root/"data/media"
                stems=[media/f"{song_id}-vocals.wav",media/f"{song_id}-instrumental.wav"]
                for dest,source in zip(stems,(speech,instrumental)):
                    dest.write_bytes(source.read_bytes())
                stem_hashes={path.name:digest(path) for path in stems}
                results=[]
                for result in (indexed,no_index):
                    entry=result["voices"][0]
                    origin_id=entry["id"]
                    installed=mcp("rvc_catalog_install",{"id":origin_id},timeout=600)
                    assert mcp("rvc_catalog_install",{"id":origin_id},timeout=600)["id"]==installed["id"]
                    assert mcp("rvc_voice_select",dict(voice_id=installed["id"],backend="audio.cpp"))["voice_id"]==installed["id"]
                    assert mcp("rvc_voice_selection_get",{})["voice_id"]==installed["id"]
                    voice_dir=root/"data/rvc/voices"/installed["id"]
                    assert digest(voice_dir/"voice.pth")==entry["weights"]["sha256"]
                    assert (voice_dir/"retrieval.index").is_file()==entry["has_index"]
                    local_weight=root/f"{origin_id}-import.pth"
                    local_weight.write_bytes((voice_dir/"voice.pth").read_bytes())
                    import_args={"model_path":str(local_weight)}
                    if entry["has_index"]:
                        assert digest(voice_dir/"retrieval.index")==entry["index"]["sha256"]
                        local_index=root/f"{origin_id}-import.index"
                        local_index.write_bytes((voice_dir/"retrieval.index").read_bytes())
                        import_args["index_path"]=str(local_index)
                    applied=mcp("rvc_apply",dict(song_id=song_id,voice_id=installed["id"],backend="audio.cpp",retrieval_blend=.35),timeout=300)
                    assert applied["effective_backend"]=="audio.cpp"
                    rendered=read(applied["audio_url"])[1]
                    assert wave_shape(rendered)[1:]==(2,rate,32,frames)
                    saved=mcp("library_song_get",dict(song_id=applied["song_id"],response_format="detailed"))
                    settings=saved["metadata"]["derived"]["settings"]
                    assert settings["retrieval_blend"]==(.35 if entry["has_index"] else 0)
                    mcp("rvc_voice_remove",{"id":installed["id"]})
                    assert not voice_dir.exists()
                    assert all(v["id"]!=installed["id"] for v in mcp("rvc_voices_list",{}))
                    assert read(applied["audio_url"])[1]==rendered
                    imported=mcp("rvc_voice_import",import_args)
                    assert imported["id"]==installed["id"]
                    assert mcp("rvc_voice_import",import_args)["id"]==installed["id"]
                    results.append(dict(catalog_id=origin_id,installed_id=installed["id"],has_index=entry["has_index"],
                        origin=installed["origin"],weights_sha256=settings["voice_sha256"],
                        index_sha256=settings["index_sha256"],take=applied,frames=frames,
                        rendered_sha256=hashlib.sha256(rendered).hexdigest(),retrieval_blend=settings["retrieval_blend"],
                        removed_files=True,reimport_same_id=True,saved_take_preserved=True))
                assert read("/v1/library/media/"+song_id)[1]==original_bytes
                assert stem_hashes=={path.name:digest(path) for path in stems}
                assert len(json.loads(read("/v1/library/songs")[1]))==3
                runtimes=[subprocess.check_output([str(exe),"--version"],cwd=exe.parent,timeout=20,text=True).strip()
                          for exe in (root/"data").rglob("audiocpp_server.exe")]
                assert runtimes,runtimes
                evidence=dict(revision=subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),
                    demo=dict(id=DEMO,sha256=example["sample"]["sha256"],size=len(contents),published_total=37,
                        not_installed=True,source=example["sample"]["url"]),sample_unavailable=[INDEXED,BARE],
                    runtime=runtimes,results=results,source_preserved=True,stems_preserved=stem_hashes)
                out=json.dumps(evidence,ensure_ascii=True,indent=2)
                Path("rvc-catalog-demos-mcp.json").write_text(out,encoding="utf-8")
                print(out)
            finally:
                if proc.poll() is None:
                    subprocess.run(["taskkill","/PID",str(proc.pid),"/T","/F"],capture_output=True)
                proc.wait(timeout=20)
if __name__=="__main__":
    run()
