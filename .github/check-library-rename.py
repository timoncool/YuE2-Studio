from check_takes import check_takes
from check_adapters import check_adapter_import
import array
import concurrent.futures
import json
import math
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
from urllib.request import Request,urlopen
import wave
import imageio_ffmpeg
from playwright.sync_api import sync_playwright

repo=Path.cwd()
root=Path(tempfile.mkdtemp())
def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1",0))
        return s.getsockname()[1]
service_port,engine_port,ui_port=free_port(),free_port(),free_port()
api=f"http://127.0.0.1:{service_port}"
ui=f"http://127.0.0.1:{ui_port}"
env=dict(os.environ,YUE_STUDIO_PORT=str(service_port),YUE_ENGINE_PORT=str(engine_port),YUE_STUDIO_DATA_ROOT=str(root),
         YUE_ENGINE_BIN=str(root/"missing.exe"),YUE_MODELS_ROOT=str(root/"models/yue2-cpp"),
         YUE_STUDIO_SETTINGS_PATH=str(root/"studio-settings.json"))
def http(path,data=None,method="GET"):
    req=Request(api+path,data=json.dumps(data).encode() if data is not None else None,method=method,headers={"Content-Type":"application/json"})
    with urlopen(req,timeout=45) as r: return json.load(r)
def wait(url,process):
    for _ in range(150):
        if process.poll() is not None: raise RuntimeError((url,process.returncode))
        try:
            with urlopen(url,timeout=2): return
        except OSError: time.sleep(.25)
    raise TimeoutError(url)
def embedded():
    tags=subprocess.check_output([imageio_ffmpeg.get_ffmpeg_exe(),"-v","error","-i",str(audio),"-f","ffmetadata","-"],text=True)
    return next(l.split("=",1)[1] for l in tags.splitlines() if l.lower().startswith("title="))
(root/"media").mkdir()
audio=root/"media"/"qa.wav"
with wave.open(str(audio),"wb") as wav:
    wav.setparams((1,2,24000,0,"NONE","not compressed"))
    wav.writeframes(array.array("h",(int((24000 if i%2003<4 else 2000)*math.sin(i*2*math.pi*220/24000)) for i in range(48000))).tobytes())
server=subprocess.Popen([str(repo/"target/debug/music-server.exe")],cwd=root,env=env)
try:
    wait(api+"/health",server)
    settings={"seed":9007199254740993,"lm_seed":9223372036854775807}
    row=http("/v1/library/songs",dict(title="Original fixture",source="external_import",metadata={"tags":["probe"]},
        generation_settings=settings,replay_request=settings,profile_id="native-q8",engine_id="ace-step",lyrics="Words",
        caption="Style",audio_path=str(audio)),"POST")
    route="/v1/library/songs/"+row["id"]
    def check(title):
        actual=http(route)
        assert actual["title"]==title and embedded()==title,(title,actual["title"],embedded())
        for field in ("source","generation_settings","profile_id","engine_id","caption","lyrics","audio_path","metadata","replay_request"):
            assert actual.get(field)==row.get(field),(field,actual.get(field),row.get(field))
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(lambda i:http(route+"/title",{"title":f"Take {i}"},"PUT"),range(8)))
    check(http(route)["title"])
    # Exercise the install button's endpoint with an existing RoFormer weight and no runtime.
    import hashlib
    expected_archive=3006666
    http("/v1/separation/settings",dict(engine="roformer",runtime="cpu",stems=["vocals","instrumental"]),"PUT")
    status=lambda:http("/v1/separation/status")
    before=status()
    assert before["missing_bytes"]==251707744+expected_archive and not before["model"]["installed"],before
    weight=root/"separation/models/bsroformer/voc_fv6-Q8_0.gguf"
    weight.parent.mkdir(parents=True,exist_ok=True)
    subprocess.run(["curl.exe","-fsSL","--retry","3","https://huggingface.co/chenmozhijin/BSRoformer-GGUF/resolve/df802a6773d25ba6ef785ff619daa3e510503168/GaboxR67/MelBandRoformers/melbandroformers/vocals/voc_fv6-Q8_0.gguf","-o",str(weight)],check=True)
    with weight.open("rb") as fp:
        assert hashlib.file_digest(fp,"sha256").hexdigest()=="2cd84c9f24513749b0cb1a6ab3e3be5c5e2f7d0e8533e50512c1f394d2828a73"
    partial=status()
    assert partial["model"]["installed"] and not partial["runtime_installed"] and partial["missing_bytes"]==expected_archive,partial
    assert http("/v1/separation/install",{},"POST")["started"]
    deadline=time.monotonic()+160
    while time.monotonic()<deadline:
        installed=status()
        if installed["ready"]:break
        assert not (installed.get("download") or {}).get("error"),installed
        time.sleep(.5)
    assert installed["runtime_installed"] and not installed["missing_bytes"],installed
    assert http(route+"/stems",{},"POST")["started"]
    deadline=time.monotonic()+180
    while time.monotonic()<deadline:
        result=http(route+"/stems")
        if (result.get("run") or {}).get("done"):break
        time.sleep(.5)
    assert set(result["stems"])=={"vocals","instrumental"},result
    assert {x["stem"] for x in result["library_songs"]}==set(result["stems"]),result
    for stem in result["stems"]:
        with urlopen(api+route+"/stems/"+stem,timeout=8) as res:
            with wave.open(__import__("io").BytesIO(res.read())) as wav:
                assert wav.getnchannels()==2 and wav.getframerate()==44100 and wav.getnframes()>80000
    http("/v1/separation/settings",dict(engine="htdemucs",runtime="cpu",stems=["vocals"]),"PUT")
    other=status()
    assert not other["model"]["installed"] and not other["runtime_installed"] and not other["ready"],other
    print(json.dumps(dict(roformer_missing_before=before["missing_bytes"],with_model=partial["missing_bytes"],after=installed["missing_bytes"],stems=result["stems"],htdemucs_ready=other["ready"])),flush=True)
    result=http("/mcp",{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"library_song_rename",
                "arguments":{"song_id":row["id"],"title":"MCP title"}}},"POST")
    assert not result["result"].get("isError"),result
    check("MCP title")
    vite=subprocess.Popen(["node","node_modules/vite/bin/vite.js","--host","127.0.0.1","--port",str(ui_port),"--strictPort"],
                          cwd=repo/"app",env=dict(os.environ,VITE_STUDIO_API_BASE=api))
    try:
        wait(ui,vite)
        with sync_playwright() as playwright:
            browser=playwright.chromium.launch(channel="msedge",headless=True)
            page=browser.new_page(viewport={"width":1600,"height":900},locale="en-US")
            page.goto(ui,wait_until="domcontentloaded")
            page.get_by_title("Studio tools",exact=True).first.click()
            panel=page.locator("section").filter(has=page.get_by_role("button",name="Processor",exact=True)).first
            device=panel.get_by_role("button",name="Auto",exact=True)
            install_button=panel.get_by_role("button",name="Download",exact=False)
            from playwright.sync_api import expect
            expect(install_button).to_be_enabled()
            pending=[]
            page.route("**/v1/separation/settings",lambda route:pending.append(route) if route.request.method=="PUT" else route.continue_())
            with page.expect_request(lambda r:r.method=="PUT" and r.url.endswith("/v1/separation/settings")):
                device.click()
            expect(install_button).to_be_disabled()
            expect(device).to_be_disabled()
            assert len(pending)==1
            pending.pop().fulfill(status=503,json={"error":"QA settings unavailable"})
            expect(page.get_by_text("QA settings unavailable",exact=True)).to_be_visible()
            expect(install_button).to_be_enabled()
            assert http("/v1/separation/settings")["runtime"]=="cpu"
            page.unroute("**/v1/separation/settings")
            with page.expect_response(lambda r:r.request.method=="PUT" and r.url.endswith("/v1/separation/settings")) as saved:
                device.click()
            assert saved.value.ok
            expect(install_button).to_be_enabled()
            assert http("/v1/separation/settings")["runtime"]=="auto"
            print("Verified pending, failed and successful device changes in the rendered stems tool",flush=True)
            page.get_by_title("Library",exact=True).first.click()
            for which,title in (("list","List title"),("details","Details title")):
                if which=="details": page.get_by_role("button",name="Song Details",exact=True).click()
                button=page.get_by_title("Rename song",exact=True)
                if which=="details":
                    page.wait_for_function("""()=>[...document.querySelectorAll('button[title="Rename song"]')].filter(el=>el.offsetWidth).length>=2""")
                (button.first if which=="list" else button.last).click()
                edit=page.locator("input:focus")
                edit.fill(title)
                with page.expect_response(lambda r:r.request.method=="PUT" and r.url==api+route+"/title") as sent:
                    edit.press("Enter")
                assert sent.value.status==200
                assert sent.value.request.post_data_json=={"title":title}
                check(title)
                print(which,title,"verified database, provenance, seeds and embedded WAV title",flush=True)
            def window_tool(name, arguments):
                reply=http("/mcp",{"jsonrpc":"2.0","id":7,"method":"tools/call",
                                   "params":{"name":name,"arguments":arguments}},"POST")["result"]
                assert not reply.get("isError"), reply
                return reply["content"][0]["text"]

            def verify_page(title):
                page.get_by_role("dialog").get_by_role("heading",name=title,exact=True).wait_for()
                report=window_tool("ui_read_page",{})
                assert report.splitlines()[0]=="Page: "+title,report[:400]

            window_tool("ui_open_settings",{"section":"interface"})
            verify_page("Appearance")
            page.get_by_role("dialog").get_by_role("navigation").get_by_role("button",name="Account",exact=False).click()
            verify_page("Account")
            window_tool("ui_open_settings",{"section":"interface"})
            verify_page("Appearance")
            window_tool("ui_open_settings",{"section":"models"})
            verify_page("Models")
            print("Verified rendered headings and MCP after Appearance -> manual Account -> MCP Appearance -> Models",flush=True)
    finally:
        vite.terminate()
        vite.wait(timeout=20)
    import shutil
    take_file=audio.with_name("processing.wav")
    shutil.copyfile(audio,take_file)
    check_adapter_import(api,root,root,False)
    result=check_takes(api,take_file,False)
    assert result["versions"][0][2]!=result["versions"][1][2],result
    print(json.dumps(result),flush=True)
    import runpy
    artwork_file=audio.with_name("artwork.wav")
    shutil.copyfile(audio,artwork_file)
    artwork=runpy.run_path(str(repo/".github/check-yue-locked-cover.py"))
    print(json.dumps(artwork["exercise"](api,artwork_file,False)),flush=True)
    # Mirror a legacy MIDI sidecar shape without publishing a user's transcription.
    mid=audio.parent/(row["id"]+".mid")
    mid.write_bytes(bytes.fromhex("4d546864000000060000000100604d54726b0000000c00903c5060803c0000ff2f00"))
    notes=[dict(start=i/10,end=(i+1)/10,pitch=60+i%12,instrument="acoustic_piano") for i in range(4179)]
    Path(str(mid)+".json").write_text(json.dumps(dict(size="medium",made_at="1",instruments=["acoustic_piano"],notes=notes)))
    args=dict(song_id=row["id"],response_format="detailed")
    reconstructed=[]
    pages=0
    while len(reconstructed)<len(notes):
        answer=http("/mcp",{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"midi_get","arguments":args}},"POST")["result"]
        page=answer["structuredContent"]
        assert json.loads(answer["content"][0]["text"])==page
        assert page["offset"]==len(reconstructed) and page["notes_count"]==len(notes) and page["notes"]
        reconstructed.extend(page["notes"])
        pages+=1
        if len(reconstructed)<len(notes): assert page["next_offset"]==len(reconstructed)
        else: assert page["next_offset"] is None
        args=dict(song_id=row["id"],offset=page["next_offset"])
    assert reconstructed==notes
    print(json.dumps(dict(midi_pages=pages,notes=len(notes))),flush=True)

finally:
    server.terminate()
    server.wait(timeout=20)
