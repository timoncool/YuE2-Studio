import concurrent.futures, hashlib, json, os, pathlib, socket, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request, wave

exe = pathlib.Path(sys.argv[1]).resolve(strict=True)
iterations = int(sys.argv[2]) if len(sys.argv)>2 else 45
with tempfile.TemporaryDirectory(prefix="library-retag-race-") as scratch:
    root = pathlib.Path(scratch)
    media = root / "data/media"
    media.mkdir(parents=True)
    wav_path = media / "fixture.wav"
    pcm = b"\x01\x00\xff\xff" * (2**20)
    with wave.open(str(wav_path), "wb") as wav:
        wav.setparams((2,2,48000,0,"NONE","not compressed"))
        wav.writeframes(pcm)
    with socket.socket() as probe:
        probe.bind(("127.0.0.1",0))
        port = probe.getsockname()[1]
    env=dict(os.environ,YUE_STUDIO_PORT=str(port),YUE_STUDIO_DATA_ROOT=str(root/"data"),
             YUE_ENGINE_BIN=str(root/"no-engine.exe"),TIMONCOOL_SHARED_DIR=str(root/"registry"),DO_NOT_TRACK="1")
    client=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def request(path, method="GET", payload=None, headers=None, prefix=None):
        request=urllib.request.Request(f"http://127.0.0.1:{port}{path}",data=json.dumps(payload).encode() if payload is not None else None,
                    method=method,headers={"Content-Type":"application/json", **(headers or {})})
        try:
            with client.open(request,timeout=10) as response:
                try:
                    return response.status, dict(response.headers), response.read(prefix)
                except __import__("http.client", fromlist=["IncompleteRead"]).IncompleteRead as error:
                    return response.status, dict(response.headers), error.partial
        except urllib.error.HTTPError as response:
            return response.code, dict(response.headers), response.read()
    with (root/"service.log").open("w") as log:
        proc=subprocess.Popen([str(exe)],env=env,stdout=log,stderr=subprocess.STDOUT)
    stop=threading.Event()
    failure=[]
    samples=[]
    def reader():
        count=0
        while not stop.is_set():
            try:
                ranged = count % 2 == 0
                status, hdr, data=request("/v1/library/media/"+song_id,
                    headers={"Range":"bytes=0-15"} if ranged else {}, prefix=None if ranged else 16)
                count+=1
                if status!=(206 if ranged else 200) or len(data)!=16 or data[:4]!=b"RIFF" or data[8:12]!=b"WAVE":
                    if len(failure)<20:
                        failure.append({"status":status,"headers":hdr,"bytes":len(data),"hex":data[:32].hex(),"prefix":repr(data[:40])})
            except Exception as error:
                if len(failure)<20:
                    failure.append({"exception":repr(error)})
        samples.append(count)
    try:
        for attempt in range(150):
            try:
                if request("/health")[0]==200:break
            except OSError:pass
            time.sleep(.08)
        else: raise AssertionError((proc.poll(),(root/"service.log").read_text()))
        request("/v1/covers/look","PUT",{"keep":False})
        code,_,body=request("/v1/library/songs","POST",{"title":"Existing PCM","audio_path":"fixture.wav","engine_id":"openrouter",
                    "generation_settings":{"model":"fixture/model"}})
        assert code in (200,201),(code,body)
        song_id=json.loads(body)["id"]
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            readers=[pool.submit(reader) for _ in range(4)]
            for i in range(iterations):
                code,_,result=request(f"/v1/library/songs/{song_id}/title","PUT",{"title":"A"*(5+(i%2)*200)+str(i)})
                assert code==200,(code,result)
            stop.set()
            for task in readers:task.result()
        code,hdr,body=request(f"/v1/library/media/{song_id}")
        assert code==200 and hdr["content-type"].startswith("audio/"),(code,hdr)
        with wave.open(str(wav_path),"rb") as result:
            final=result.readframes(2**20)
        assert final==pcm
        audit={"exe":str(exe),"rounds":iterations,"range_reads":sum(samples),"final_pcm_sha256":hashlib.sha256(final).hexdigest(),
               "media_type":hdr["content-type"],"bad_responses":failure,"service_tagging_errors":[line for line in (root/"service.log").read_text(encoding="utf-8",errors="replace").splitlines() if "could not tag" in line]}
        print(json.dumps(audit,indent=2))
        sys.exit(1 if failure or audit["service_tagging_errors"] else 0)
    finally:
        stop.set(); proc.terminate(); proc.wait(timeout=15)
