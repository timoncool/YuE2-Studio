from contextlib import contextmanager

import base64, hashlib, http.server, io, json, math, os, pathlib, socket, struct, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request, wave

exe = pathlib.Path(sys.argv[1]).resolve()
model = "fixture/sse-music"
model_name = "Fixture music"
wav_buffer = io.BytesIO()
with wave.open(wav_buffer, "wb") as audio:
    audio.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
    audio.writeframes(b"".join(struct.pack("<h", int(3000 * math.sin(n * math.pi * 440 / 4000))) for n in range(8000)))
wav = wav_buffer.getvalue()
encoded = base64.b64encode(wav).decode()
actual_encoded = None
calls = []
slow_started = threading.Event()
slow_disconnected = threading.Event()

class Provider(http.server.BaseHTTPRequestHandler):
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
        wire=base64.b64encode(b"RIFF\x04\x00\x00\x00WAVE").decode() if malformed else actual_encoded if "rvc actual success case" in content else encoded
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
    (root/"openrouter-catalog-v2.json").write_text(json.dumps({"models":[{"id":model,"name":model_name,"input_modalities":["text"],"output_modalities":["audio"],"supported_parameters":[],"capabilities":["music_generation"]}]}),encoding="utf8")
    port=unused_port()
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
        with client.open(req,timeout=900) as response:
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
        proc=subprocess.Popen([str(exe)],env=env,stdout=log,stderr=subprocess.STDOUT)
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
    proc=run_service()
    try:
        body={"music_source":{"kind":"openrouter","model":model},"title":"Fixture song","style":"soft piano",
              "lyrics":"A local response sings","client_ref":"fixture-client","duration_seconds":6,"seed":42}
        rpc("initialize",{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"openrouter-sse-qa","version":"1"}},1)
        rpc("notifications/initialized",{})
        tools=rpc("tools/list",{},2)["tools"]
        song_create=next(tool for tool in tools if tool["name"]=="song_create")
        sources=song_create["inputSchema"]["properties"]["music_source"]["oneOf"]
        assert {source["properties"]["kind"]["const"] for source in sources}=={"local","native","audio_cpp","openrouter"},sources
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
        assert calls[0]["model"]==model and calls[0]["stream"] is True,calls
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
        code,data=request("/v1/music/jobs/"+slow_job)
        assert code==200 and json.loads(data)["status"]=="cancelled" and not json.loads(data).get("song"),(code,data)
        code,data=request("/v1/library/songs")
        assert code==200 and len(json.loads(data))==1,(code,data)
        time.sleep(.35)
        assert tool("rvc_quick_list", {}, 30) == [], "cancelled generation queued RVC"

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
        assert len(calls)==4,calls
        mcp_headers.pop("Mcp-Session-Id", None)
        rpc("initialize", {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "rvc-after-qa", "version": "1"}}, 31)
        rpc("notifications/initialized", {})
        evidence = {}
        fixture = root / "source-speech.wav"
        subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", """
Add-Type -AssemblyName System.Speech
$s = [System.Speech.Synthesis.SpeechSynthesizer]::new()
$f = [System.Speech.AudioFormat.SpeechAudioFormatInfo]::new(44100, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$s.SetOutputToWaveFile($env:RVC_QA_SPEECH, $f)
$s.Speak("The morning wind is gentle. We can sing and hear the river flowing through the valley.")
$s.Dispose()
"""], env=dict(os.environ, RVC_QA_SPEECH=str(fixture)), check=True)
        actual_encoded = base64.b64encode(fixture.read_bytes()).decode()
        assert tool("rvc_apply_install", {}, 31)["backends"]["audio.cpp"]["ready"]
        tool("separator_install", {}, 32)
        voice = tool("rvc_catalog_install", {"id": "rvc-50ea3d733073550ef6f7"}, 33)
        selected = tool("rvc_voice_select", {"voice_id": voice["id"], "backend": "audio.cpp"}, 34)
        assert selected["voice_id"] == voice["id"]
        auto = tool("song_create", dict(body, title="RVC automatic fixture", style="rvc actual success case",
                    rvc_after=selected, response_format="detailed"), 35)
        completed_auto = finish(auto["id"])
        assert completed_auto["status"] == "completed", completed_auto
        original = completed_auto["song"]
        automatic_song = original["song"]["id"]
        original_wav = request(original["audio_url"])[1]
        with wave.open(io.BytesIO(original_wav)) as audio:
            original_frames = audio.readframes(audio.getnframes())
            duration = audio.getnframes() / audio.getframerate()
        for i in range(1800):
            rows = tool("rvc_quick_list", {}, 100+i)
            if rows and rows[0]["status"] in ("completed", "failed"):
                break
            time.sleep(.2)
        else:
            raise AssertionError("automatic RVC was not queued")
        assert len(rows) == 1 and rows[0]["song_id"] == automatic_song, rows
        converted = rows[0]
        assert converted["status"] == "completed", converted
        assert converted["voice_id"] == voice["id"] and converted["backend"] == "audio.cpp", converted
        assert converted["take"]["effective_backend"] == "audio.cpp", converted
        duplicate = tool("rvc_quick_start", {"song_id": automatic_song}, 2500)
        assert duplicate["id"] == converted["id"] and duplicate["take"] == converted["take"], duplicate
        source_for_cancel = root / "cancel-quick.wav"
        source_for_cancel.write_bytes(fixture.read_bytes())
        imported = tool("library_import_audio", {"path": str(source_for_cancel), "title": "Quick RVC cancellation fixture"}, 2501)
        cancel_song = imported.get("song", imported)["id"]
        running = tool("rvc_quick_start", {"song_id": cancel_song}, 2502)
        repeated = tool("rvc_quick_start", {"song_id": cancel_song}, 2503)
        assert running["id"] == repeated["id"], (running, repeated)
        stopped = tool("rvc_quick_cancel", {"id": running["id"]}, 2504)
        assert stopped["status"] == "cancelled" and stopped["take"] is None, stopped
        time.sleep(5)
        assert tool("rvc_quick_get", {"id": running["id"]}, 2505)["take"] is None
        assert assert_played_wav(complete["song"]["audio_url"]) == pcm_sha256
        check_wav = request(original["audio_url"])[1]
        with wave.open(io.BytesIO(check_wav)) as audio:
            assert audio.readframes(audio.getnframes()) == original_frames
        code, data = request("/v1/library/songs")
        library = json.loads(data)
        assert code == 200 and {row["id"] for row in library} == {song_id, cancel_song, automatic_song, converted["take"]["song_id"]}, library
        assert len(library) == 4
        evidence = {"revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                    "automatic_generation_job": auto["id"], "source": automatic_song,
                    "source_seconds": duration, "source_pcm_sha256": hashlib.sha256(original_frames).hexdigest(),
                    "source_preserved": True, "converted": converted, "repeated_result": duplicate["id"],
                    "cancelled": stopped, "library_ids": [row["id"] for row in library]}
        pathlib.Path("rvc-quick-actions-mcp.json").write_text(json.dumps(evidence, indent=2), encoding="utf8")
        print(json.dumps(evidence, indent=2))
        print(json.dumps({"status":"pass","http_calls":len(calls),"library_song":song_id,"wav_bytes":len(wav),
                          "restart_persisted":True,"incomplete_failed":True,"replay_rejected":True,
                          "explicit_model":model,"upstream_disconnected_on_cancel":True,"cancel_no_import":True,"mcp_tools_list":True,"mcp_catalog":True,"mcp_song_create":True,"model_name":model_name,"id3_artist":model,"pcm_sha256":pcm_sha256,"empty_prompt_rejected":True,"malformed_wav_rejected":True},indent=2))
    finally:
        proc.terminate()
        proc.wait(timeout=15)


