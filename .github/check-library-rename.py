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
finally:
    server.terminate()
    server.wait(timeout=20)
