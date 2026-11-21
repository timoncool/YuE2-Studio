from contextlib import contextmanager

import base64, hashlib, http.server, io, json, math, os, pathlib, socket, struct, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request, wave

exe = pathlib.Path(sys.argv[1]).resolve()
webview = "--webview" in sys.argv
catalog_phase = "error"
allow_catalog_error = threading.Event()
model = "fixture/sse-music"
model_name = "Fixture music"
peer_model = "fixture/sse-music-b"
final_model = "fixture/sse-music-c"
wav_buffer = io.BytesIO()
with wave.open(wav_buffer, "wb") as audio:
    audio.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
    audio.writeframes(b"".join(struct.pack("<h", int(3000 * math.sin(n * math.pi * 440 / 4000))) for n in range(8000)))
wav = wav_buffer.getvalue()
encoded = base64.b64encode(wav).decode()
calls = []
slow_started = threading.Event()
slow_disconnected = threading.Event()
natural_started = threading.Event()
finish_naturally = threading.Event()

class Provider(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        assert webview and self.path.startswith("/models"), self.path
        if self.path == "/models" and catalog_phase == "error":
            allow_catalog_error.wait(30)
            self.send_error(503, "fixture catalog unavailable")
            return
        models = []
        if self.path == "/models" and catalog_phase == "ready":
            models = [
                {"id": identifier, "name": name, "description": "Generate full-length songs from text.",
                 "architecture": {"input_modalities": ["text"], "output_modalities": ["audio"]}}
                for identifier, name in [(model, model_name), (peer_model, "Second fixture music model")]
            ]
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps({"data": models}).encode())

    def do_POST(self):
        assert self.path.endswith("/chat/completions"), self.path
        assert self.headers["Authorization"] == "Bearer fixture-only"
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        calls.append(body)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        content=body["messages"][0]["content"]
        malformed="broken wave case" in content
        wire=base64.b64encode(b"RIFF\x04\x00\x00\x00WAVE").decode() if malformed else encoded
        for chunk in (wire[:51], wire[51:]):
            if not chunk:
                continue
            event = {"choices": [{"delta": {"audio": {"data": chunk}}}]}
            self.wfile.write(f"data: {json.dumps(event)}\r\n\r\n".encode())
            self.wfile.flush()
        if "slow case" in body["messages"][0]["content"]:
            slow_started.set()
            try:
                for _ in range(100):
                    self.wfile.write(b": waiting " + b"x" * 32768 + b"\r\n\r\n")
                    self.wfile.flush()
                    time.sleep(.05)
            except OSError:
                slow_disconnected.set()
                return
        if "natural wait case" in content:
            natural_started.set()
            assert finish_naturally.wait(8),"the test never released the music response"
        if "incomplete case" not in body["messages"][0]["content"]:
            self.wfile.write(b"data: [DONE]\r\n\r\n")

def unused_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1",0))
        return listener.getsockname()[1]

@contextmanager
def local_provider():
    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider) as server:
        # server_close waits for request handlers once the accept loop has stopped.
        server.daemon_threads = False
        listener = threading.Thread(target=server.serve_forever, name="music-fixture")
        listener.start()
        try:
            yield server
        finally:
            server.shutdown()
            listener.join()


with tempfile.TemporaryDirectory(prefix="yue-openrouter-") as profile, local_provider() as upstream:
    root=pathlib.Path(profile)
    if not webview:
        options = ["seed", "temperature", "top_p", "max_tokens"]
        cached_models = [
            {"id": identifier, "name": model_name, "input_modalities": ["text"], "output_modalities": ["audio"],
             "supported_parameters": options if identifier == final_model else [], "capabilities": ["music_generation"]}
            for identifier in (model, peer_model, final_model)
        ]
        (root / "openrouter-catalog-v2.json").write_text(json.dumps({"models": cached_models}), encoding="utf8")
    port = 8791 if webview else unused_port()
    # Only this child process gets the disposable Studio profile and credentials.
    for folder in ("home", "appdata", "localappdata"):
        (root / folder).mkdir()
    env=dict(os.environ,YUE_STUDIO_DATA_ROOT=profile,YUE_STUDIO_PORT=str(port),YUE_ENGINE_PORT=str(unused_port()),
             YUE2_QA_OPENROUTER_PORT=str(upstream.server_port),OPENROUTER_API_KEY="must-not-send-to-fixture",
             HOME=str(root/"home"),USERPROFILE=str(root/"home"),APPDATA=str(root/"appdata"),
             LOCALAPPDATA=str(root/"localappdata"))
    env.pop("YUE_ENGINE_BASE_URL",None)
    client=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def request(path,payload=None):
        url=f"http://127.0.0.1:{port}"+path
        req=urllib.request.Request(url,data=json.dumps(payload).encode() if payload is not None else None,
                                   headers={"Content-Type":"application/json"})
        try:
            with client.open(req,timeout=8) as r: return r.status,r.read()
        except urllib.error.HTTPError as e: return e.code,e.read()
    mcp_headers={"Content-Type":"application/json","Accept":"application/json, text/event-stream"}
    def rpc(method,params,rid=None):
        body={"jsonrpc":"2.0","method":method,"params":params}
        if rid is not None:
            body["id"]=rid
        req=urllib.request.Request(f"http://127.0.0.1:{port}/mcp",data=json.dumps(body).encode(),headers=mcp_headers)
        with client.open(req,timeout=12) as response:
            raw=response.read().decode()
            if response.headers.get("Mcp-Session-Id"):
                mcp_headers["Mcp-Session-Id"]=response.headers["Mcp-Session-Id"]
        if not raw.strip():
            return {}
        if raw.startswith(("event:","data:")):
            raw=next(line[6:] for line in raw.splitlines() if line.startswith("data: "))
        message=json.loads(raw)
        assert "error" not in message,message
        return message["result"]
    def tool(name,args,rid):
        response=rpc("tools/call",{"name":name,"arguments":args},rid)
        assert not response.get("isError"),response
        return response.get("structuredContent") or json.loads(response["content"][0]["text"])
    def id3_artist(wave_bytes):
        def synchsafe(field):
            value=0
            for byte in field:
                value=(value<<7)|byte
            return value

        # Skip the actual audio and inspect only ID3 tags in RIFF chunks.
        offset=12
        while offset+8<=len(wave_bytes):
            kind,size=struct.unpack_from("<4sI",wave_bytes,offset)
            chunk=wave_bytes[offset+8:offset+8+size]
            offset+=8+size+(size&1)
            if kind.lower()!=b"id3 " or not chunk.startswith(b"ID3"):
                continue
            version=chunk[3]
            assert version in (3,4),version
            frame_offset=10
            limit=min(len(chunk),10+synchsafe(chunk[6:10]))
            while frame_offset+10<=limit:
                frame=chunk[frame_offset:frame_offset+4]
                raw_size=chunk[frame_offset+4:frame_offset+8]
                frame_size=synchsafe(raw_size) if version==4 else int.from_bytes(raw_size,"big")
                if frame_size==0:
                    break
                data=chunk[frame_offset+10:frame_offset+10+frame_size]
                if frame==b"TPE1":
                    encoding=("latin1","utf-16","utf-16-be","utf-8")[data[0]]
                    return data[1:].decode(encoding).rstrip("\0")
                frame_offset+=10+frame_size
        return None

    def assert_played_wav(url):
        # The job is marked complete before the asynchronous WAV tag rewrite.
        for _ in range(80):
            code,served=request(url)
            assert code==200,(code,served[:100],url)
            assert served[:4]==b"RIFF" and served[8:12]==b"WAVE"
            with wave.open(io.BytesIO(wav)) as original, wave.open(io.BytesIO(served)) as playback:
                assert playback.getparams()==original.getparams(),playback.getparams()
                frames=playback.readframes(8000)
                assert frames==original.readframes(8000)
            artist=id3_artist(served)
            if artist:
                assert artist==model,artist
                return hashlib.sha256(frames).hexdigest()
            time.sleep(.1)
        raise AssertionError("The library WAV never acquired its ID3 artist")
    def run_service():
        log=(root/"service.log").open("ab")
        proc=subprocess.Popen([str(exe), *(["--headless"] if webview else [])],env=env,stdout=log,stderr=subprocess.STDOUT)
        try:
            for _ in range(180):
                assert proc.poll() is None,proc.returncode
                try:
                    if request("/health")[0]==200: return proc
                except OSError: pass
                time.sleep(.25)
            raise TimeoutError("service health")
        except BaseException:
            proc.terminate()
            proc.wait()
            raise
        finally:
            log.close()
    def finish(job):
        for _ in range(120):
            code,data=request("/v1/music/jobs/"+job)
            assert code==200,data
            payload=json.loads(data)
            if payload["status"] in ("completed","failed"): return payload
            time.sleep(.1)
        raise TimeoutError("music job")
    def check_webview():
        global catalog_phase
        rpc("initialize", {"protocolVersion": "2025-03-26", "capabilities": {},
                           "clientInfo": {"name": "openrouter-webview-qa", "version": "1"}}, 1)
        rpc("notifications/initialized", {})
        next_id = 1

        def ui(name, **args):
            nonlocal next_id
            next_id += 1
            reply = rpc("tools/call", {"name": "ui_" + name, "arguments": args}, next_id)
            assert not reply.get("isError"), reply
            return reply["content"][0]["text"]

        def wait_page(*strings, any_of=()):
            last = ""
            deadline = time.monotonic() + 35
            while time.monotonic() < deadline:
                try:
                    last = ui("read_page")
                    if all(string in last for string in strings) and (not any_of or any(string in last for string in any_of)):
                        return last
                except (AssertionError, OSError) as error:
                    last = str(error)
                time.sleep(.15)
            raise AssertionError(f"Missing {strings!r} (alternatives: {any_of!r}) in WebView page:\n{last}")

        def click_button(text, page):
            line = next(line for line in page.splitlines() if f'button "{text}"' in line)
            ui("click", ref=line.split()[0])

        page = wait_page(any_of=('button "Use OpenRouter"', 'select "Model"'))
        if 'button "Use OpenRouter"' in page:
            click_button("Use OpenRouter", page)
        page = wait_page('select "Model"')
        if 'select "Model" value="openrouter"' not in page:
            ui("select", text="Model", value="openrouter")
        wait_page('status "Loading OpenRouter models')
        allow_catalog_error.set()
        page = wait_page('alert "Could not load OpenRouter music models."', 'button "Refresh models"')
        catalog_phase = "empty"
        click_button("Refresh models", page)
        page = wait_page('status "No OpenRouter music models are available."', 'button "Refresh models"')
        assert 'button "Create" disabled' in page
        catalog_phase = "ready"
        click_button("Refresh models", page)
        wait_page("cloud:" + model, "cloud:" + peer_model)
        # Exercise changing an already suggested/selected model in the single Create form.
        ui("select", text="Model", value="cloud:" + model)
        ui("select", text="Model", value="cloud:" + peer_model)
        tag = "OpenRouter WebView fixture song"
        prompt = "Warm piano with a brushed snare"
        ui("type", text="Untitled", value=tag)
        ui("type", text="Style", value=prompt)
        page = wait_page(f'value="cloud:{peer_model}"', f'value="{prompt}"')
        # Sidebar Create is listed first. The submit button is inside the form.
        submits = [line for line in page.splitlines() if line.endswith('button "Create"')]
        assert len(submits) >= 2, page
        ui("click", ref=submits[-1].split()[0])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            code, data = request("/v1/library/songs")
            assert code == 200, data
            songs = json.loads(data)
            song = next((item for item in songs if item["title"] == tag), None)
            if song:
                break
            time.sleep(.2)
        else:
            raise AssertionError(f"WebView song did not reach the library; upstream calls={calls!r}")
        assert len(calls) == 1 and calls[0]["model"] == peer_model and calls[0]["stream"] is True, calls
        assert song["engine_id"] == "openrouter" and song["generation_settings"]["model"] == peer_model, song
        code, payload = request("/v1/library/media/" + song["id"])
        assert code == 200, code
        with wave.open(io.BytesIO(payload)) as playback, wave.open(io.BytesIO(wav)) as original:
            pcm = playback.readframes(8000)
            assert pcm == original.readframes(8000)
        ui("select", text="Model", value="local")
        wait_page('select "Model" value="local"', f'value="{prompt}"')
        receipt = {"transport": "Windows WebView2 and localhost SSE", "catalog": ["loading", "error", "empty", "ready"],
                   "model": peer_model, "library_id": song["id"], "pcm_sha256": hashlib.sha256(pcm).hexdigest(),
                   "local_draft_restored": True, "paid_requests": 0}
        print(json.dumps(receipt, indent=2))
        local_style = "make it massive, without drums"
        updated = rpc("tools/call", {"name": "create_form_set", "arguments": {"fields": {"style": local_style}}}, next_id + 1)
        assert not updated.get("isError"), updated
        wait_page(f'value="{local_style}"')
        form = tool("create_form_get", {}, next_id + 20)
        assert form["fields"]["style"] == local_style, form
        hint_rules = {hint["rule"] for hint in form["style_hints"]}
        assert {"negative", "instruction"} <= hint_rules, form
        print(json.dumps({"style": local_style, "style_hints": sorted(hint_rules)}, indent=2))

    proc=run_service()
    try:
        if webview:
            check_webview()
            sys.exit(0)
        body={"music_source":{"kind":"openrouter","model":model},"title":"Fixture song","style":"soft piano",
              "lyrics":"A local response sings","client_ref":"fixture-client","duration_seconds":6,"seed":42}
        rpc("initialize",{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"openrouter-sse-qa","version":"1"}},1)
        rpc("notifications/initialized",{})
        tools=rpc("tools/list",{},2)["tools"]
        song_create=next(tool for tool in tools if tool["name"]=="song_create")
        sources=song_create["inputSchema"]["properties"]["music_source"]["oneOf"]
        assert {source["properties"]["kind"]["const"] for source in sources}=={"local","native","signed","audio_cpp","openrouter"},sources
        status, data = request("/v1/capabilities")
        assert status == 200, (status, data)
        provider = next(engine for engine in json.loads(data)["engines"] if engine["id"] == "openrouter")
        assert "music_generation" in provider["capabilities"], provider
        catalog=tool("openrouter_catalog",{},3)
        assert any(item["id"]==model and "music_generation" in item["capabilities"] for item in catalog["models"]),catalog
        submitted=tool("song_create",dict(body,response_format="detailed"),4)
        assert submitted["dispatch"]=="open_router" and submitted["client_ref"]=="fixture-client",submitted
        complete=finish(submitted["id"])
        assert complete["status"]=="completed",complete
        song=complete["song"]["song"]
        song_id=song["id"]
        assert complete["duration_seconds"]==1.0 and song["metadata"]["duration_seconds"]==1.0,complete
        assert song["title"]=="Fixture song" and song["caption"]=="soft piano" and song["lyrics"]==body["lyrics"],song
        assert song["generation_settings"]["request"]["seed"]==42 and song["generation_settings"]["model"]==model,song
        assert song["generation_settings"]["model_name"]==model_name,song
        assert song["engine_id"]=="openrouter" and not song.get("replay_request") and not song.get("audio_codes"),song
        assert not song["generation_settings"].get("api_key"),song
        assert calls[0]["model"]==model and calls[0]["stream"] is True and "seed" not in calls[0],calls
        assert "soft piano" in calls[0]["messages"][0]["content"] and body["lyrics"] in calls[0]["messages"][0]["content"],calls
        pcm_sha256=assert_played_wav(complete["song"]["audio_url"])
        code,data=request("/v1/music/replay",{"song_id":song_id})
        assert code==400 and "no replay request" in data.decode(),(code,data)
        code,data=request("/v1/music/jobs",dict(body,semantic_tokens="1,2,3"))
        assert code==400 and len(calls)==1,(code,data,calls)
        code,data=request("/v1/music/jobs",dict(body,style="incomplete case"))
        assert code==202,(code,data)
        failed=finish(json.loads(data)["id"])
        assert failed["status"]=="failed" and not failed.get("song") and "[DONE]" in failed["message"],failed
        assert len(calls)==2,calls
        code,data=request("/v1/music/jobs",dict(body,style="  ",lyrics="\n"))
        assert code==400 and len(calls)==2,(code,data,calls)
        code,data=request("/v1/music/jobs",dict(body,title="Broken WAV",style="broken wave case"))
        assert code==202,(code,data)
        failed=finish(json.loads(data)["id"])
        assert failed["status"]=="failed" and not failed.get("song"),failed
        assert len(calls)==3,calls
        voice_id = "a" * 64
        rvc_after = {"voice_id": voice_id, "backend": "audio.cpp"}
        code,data=request("/v1/music/jobs",dict(body,title="Must not be stored",style="slow case",rvc_after=rvc_after))
        assert code==202,(code,data)
        slow_job=json.loads(data)["id"]
        assert slow_started.wait(5),"provider never started its stream"
        source_for = lambda model_id: {"kind":"openrouter","model":model_id}
        def select_pair(model_id, sequence):
            return tool("music_pair_select", {"music_source":source_for(model_id),
                        "client_id":"fixture-create-window","sequence":sequence}, 100 + sequence)
        def wait_for_pair(predicate):
            for _ in range(120):
                current = tool("music_pair_status",{},120)
                if predicate(current):
                    return current
                time.sleep(.05)
            raise AssertionError(current)
        pair_b = select_pair(peer_model, 1)
        duplicate_b = select_pair(peer_model, 2)
        assert duplicate_b["revision"] == pair_b["revision"], duplicate_b
        pair_c = select_pair(final_model, 4)
        late_b = select_pair(peer_model, 3)
        assert late_b["revision"] == pair_c["revision"] and late_b["requested"] == source_for(final_model), late_b
        waiting = wait_for_pair(lambda pair:pair["stage"] == "waiting_for_generation")
        assert waiting["waiting_job_id"] == slow_job and waiting["active"] == source_for(model), waiting
        assert waiting["requested"] == source_for(final_model), waiting
        code,reconnected = request("/v1/music/pair")
        assert code == 200 and json.loads(reconnected)["revision"] == pair_c["revision"], reconnected
        assert len(calls) == 4,calls
        if sys.platform in ("win32", "linux") and os.environ.get("PROCESSOR_ARCHITECTURE", "AMD64").lower() in ("amd64", "x86_64"):
            native=root/"rvc"/"native"
            native_runtime=native/"v1"
            native_runtime.mkdir(parents=True)
            executable="vocal-fast.exe" if sys.platform=="win32" else "vocal-fast"
            for filename in (executable, "hubert.vfrm", "rmvpe.vfrm"):
                (native_runtime/filename).write_bytes(b"passive discovery fixture")
            assert tool("rvc_catalog_list", {"query":"Radio Tapok"}, 20)["total"] > 0
            assert tool("rvc_voices_list", {}, 21) == []
            detected=tool("rvc_backend_status", {}, 22)["backends"]["vocal-fast-rs"]
            assert not detected["can_start"] and not detected["ready"], detected
            installer = tool("rvc_native_runtime_status", {}, 23)
            assert not installer["installed"] and installer["download"] is None, installer
            assert not (native/"cache-v1").exists(), "reading the catalog started native RVC"
            code,data=request("/v1/music/jobs/"+slow_job)
            assert code==200 and json.loads(data)["status"]=="running",(code,data)

        voice_folder = root / "rvc" / "voices" / voice_id
        voice_folder.mkdir(parents=True)
        (voice_folder / "voice.pth").write_bytes(b"never dispatched to a backend")
        (voice_folder / "voice.json").write_text(json.dumps({
            "id": voice_id, "model_name": "fixture.pth", "index_name": None, "version": 2,
        }), encoding="utf8")
        code,data=request("/v1/music/jobs/"+slow_job,{})
        assert code==200 and json.loads(data)["status"]=="cancelled",(code,data)
        assert slow_disconnected.wait(5),"cancel did not close the upstream HTTP connection"
        prepared = wait_for_pair(lambda pair:pair["stage"] == "ready")
        assert prepared["active"] == source_for(final_model) and prepared["load_on_create"], prepared
        assert prepared["revision"] == pair_c["revision"] and len(calls)==4,prepared
        same = select_pair(final_model, 5)
        assert same["revision"] == prepared["revision"],same
        code,data=request("/v1/music/jobs/"+slow_job)
        assert code==200 and json.loads(data)["status"]=="cancelled" and not json.loads(data).get("song"),(code,data)
        code,data=request("/v1/library/songs")
        assert code==200 and len(json.loads(data))==1,(code,data)
        time.sleep(.35)
        assert tool("rvc_quick_list", {}, 30) == [], "cancelled generation queued RVC"
        options = {"seed": 0, "temperature": 0.0, "top_p": 0.8, "max_tokens": 4096}
        submission = tool("song_create",
                          dict(body, title="Final paired source", music_source=source_for(final_model),
                               openrouter=options, response_format="detailed"), 41)
        final = finish(submission["id"])
        assert final["song"]["song"]["generation_settings"]["request"]["openrouter"] == options, final
        assert {key: calls[-1][key] for key in options} == options, calls[-1]
        assert final["status"] == "completed" and calls[-1]["model"] == final_model,(final,calls[-1])
        assert len(calls) == 5

        code,data=request("/v1/music/jobs",dict(body,title="Keep the song",style="natural wait case",music_source=source_for(final_model)))
        assert code == 202,(code,data)
        natural_id = json.loads(data)["id"]
        try:
            assert natural_started.wait(4),"provider never started the song that should finish"
            wanted_b = select_pair(peer_model, 6)
            waiting = wait_for_pair(lambda pair:pair["stage"] == "waiting_for_generation")
            assert waiting["waiting_job_id"] == natural_id and waiting["active"] == source_for(final_model), waiting
            assert waiting["revision"] == wanted_b["revision"], waiting
        finally:
            finish_naturally.set()
        naturally_finished = finish(natural_id)
        assert naturally_finished["status"] == "completed" and naturally_finished["song"]["audio_url"], naturally_finished
        prepared_b = wait_for_pair(lambda pair:pair["stage"] == "ready")
        assert prepared_b["active"] == source_for(peer_model), prepared_b
        error = tool("music_pair_select", {"music_source":{"kind":"native","engine":"unknown","model":"fixture"}}, 141)
        failed_pair = wait_for_pair(lambda pair:pair["stage"] == "error")
        assert failed_pair["revision"] == error["revision"] and failed_pair["active"] == source_for(peer_model), failed_pair
        missing = tool("music_pair_select", {"music_source":{"kind":"signed","engine":"missing-engine","model":"missing-model"}}, 142)
        needs_install = wait_for_pair(lambda pair:pair["stage"] == "needs_install")
        assert needs_install["revision"] == missing["revision"] and needs_install["active"] == source_for(peer_model), needs_install
        assert len(calls) == 6
        print("PASS music pair A -> B -> C, duplicate and reordered requests, reconnect, explicit cancellation and final provider dispatch")

    finally:
        proc.terminate()
        proc.wait(timeout=15)
    proc=run_service()
    try:
        code,data=request("/v1/library/songs/"+song_id)
        assert code==200,(code,data)
        restored=json.loads(data)
        assert restored["id"]==song_id and restored["generation_settings"]["model_name"]==model_name,restored
        assert restored["generation_settings"]["model"]==model,restored
        assert assert_played_wav(complete["song"]["audio_url"])==pcm_sha256
        assert len(calls)==6,calls
        mcp_headers.pop("Mcp-Session-Id", None)
        rpc("initialize", {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "rvc-after-qa", "version": "1"}}, 31)
        rpc("notifications/initialized", {})
        auto = tool("song_create", dict(body, title="RVC automatic fixture", rvc_after=rvc_after, response_format="detailed"), 31)
        completed_auto = finish(auto["id"])
        assert completed_auto["status"] == "completed", completed_auto
        automatic_song = completed_auto["song"]["song"]["id"]
        for i in range(100):
            rows = tool("rvc_quick_list", {}, 100+i)
            if rows and rows[0]["status"] == "failed":
                break
            time.sleep(.1)
        else:
            raise AssertionError("automatic RVC was not queued")
        assert len(rows) == 1 and rows[0]["song_id"] == automatic_song, rows
        assert rows[0]["voice_id"] == voice_id and rows[0]["backend"] == "audio.cpp", rows[0]
        assert rows[0]["take"] is None and "Settings" in rows[0]["error"], rows[0]
        assert assert_played_wav(completed_auto["song"]["audio_url"]) == pcm_sha256

        print(json.dumps({"status":"pass","http_calls":len(calls),"library_song":song_id,"wav_bytes":len(wav),
                          "restart_persisted":True,"music_pair_lifecycle":True,"incomplete_failed":True,"replay_rejected":True,
                          "explicit_model":model,"upstream_disconnected_on_cancel":True,"cancel_no_import":True,"mcp_tools_list":True,"mcp_catalog":True,"mcp_song_create":True,"model_name":model_name,"id3_artist":model,"pcm_sha256":pcm_sha256,"empty_prompt_rejected":True,"malformed_wav_rejected":True},indent=2))
    finally:
        proc.terminate()
        proc.wait(timeout=15)


