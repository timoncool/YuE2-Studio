"""Drive only the bundled Studio WebView on the isolated Windows runner."""
import argparse, base64, ctypes, json, os, pathlib, re, struct, subprocess, tempfile, time, urllib.request, wave, math

p = argparse.ArgumentParser()
p.add_argument("exe", type=pathlib.Path)
p.add_argument("output", type=pathlib.Path)
p.add_argument("--suite", choices=["pt", "screens"], default="pt")
args = p.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
base = "http://127.0.0.1:8791"
trace = []
session = None
calls = 0

def json_file(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf8")

def request(path, body=None, method=None, rpc=False):
    global session
    headers = {"Content-Type": "application/json"}
    if rpc:
        headers["Accept"] = "application/json, text/event-stream"
        if session:
            headers["Mcp-Session-Id"] = session
    req = urllib.request.Request(base + path, method=method,
        data=json.dumps(body).encode() if body is not None else None, headers=headers)
    with client.open(req, timeout=48 if rpc else 8) as reply:
        session = reply.headers.get("Mcp-Session-Id", session)
        return reply.read()

def rpc(method, params, notify=False):
    global calls
    calls += 1
    body = dict(jsonrpc="2.0", method=method, params=params)
    if not notify:
        body["id"] = calls
    raw = request("/mcp", body, rpc=True).decode()
    if notify:
        return
    if raw.startswith(("event:", "data:")):
        raw = next(line[6:] for line in raw.splitlines() if line.startswith("data: "))
    result = json.loads(raw)
    assert not result.get("error"), result
    return result["result"]

def tool(tool_name, /, **fields):
    result = rpc("tools/call", dict(name=tool_name, arguments=fields))
    assert not result.get("isError"), (tool_name, fields, result)
    content = result.get("content", [])
    if tool_name == "ui_screenshot":
        return next(item for item in content if item.get("type") == "image")["data"]
    answer = result.get("structuredContent") or content[0]["text"]
    if isinstance(answer, str) and answer.startswith(("{","[")):
        try:
            answer = json.loads(answer)
        except ValueError:
            pass
    trace.append(dict(tool=tool_name, fields=fields, answer=answer))
    return answer

def ui(action, /, **fields):
    return tool("ui_" + action, **fields)

def wait_page(*terms, predicate=None):
    last = ""
    deadline = time.monotonic()+35
    while time.monotonic() < deadline:
        try:
            last = ui("read_page")
            if all(term in last for term in terms) and (predicate is None or predicate(last)):
                return last
        except (OSError, AssertionError) as problem:
            last = str(problem)
        time.sleep(.25)
    raise AssertionError(f"Missing {terms!r} in page:\n{last}")

def control(page, text):
    line = next((line for line in page.splitlines() if line.startswith("e") and text in line), None)
    assert line, (text, page)
    return line.split()[0]

def click(text, page=None):
    return ui("click", ref=control(page or wait_page(text), text))

def screenshot(name):
    pixels = base64.b64decode(ui("screenshot", max_width=1440))
    assert pixels.startswith(b"\x89PNG\r\n\x1a\n")
    size = struct.unpack(">II", pixels[16:24])
    assert size[0] >= 1000 and size[1] >= 700 and len(pixels) > 10_000, size
    (out / (name + ".png")).write_bytes(pixels)
    trace.append(dict(screenshot=name, pixels=size, bytes=len(pixels)))

def seed_training(root):
    stamp = "2026-11-21T07:00:00Z"
    dataset = "demo-dataset"
    json_file(root/"training"/"datasets"/dataset/"dataset.json",
              dict(id=dataset, name="Canções de exemplo", created_at=stamp))
    run = root/"training"/"runs"/"demo-run"
    json_file(run/"run.json", dict(id="demo-run", engine="yue2-cpp", dataset_id=dataset,
        dataset_name="Canções de exemplo", name="LoRA de exemplo", trigger="lodemo",
        recipe=dict(preset="balanced", steps=50, save_every=50), status="done",
        stage=None, stages=["train"], steps=[dict(step=i, loss=round(1/i**.35,4), ar_kl=round(i/1000,4))
        for i in range(1,51)], created_at=stamp, finished_at=stamp))
    checkpoint = run/"output"/"checkpoint-step50"
    checkpoint.mkdir(parents=True)
    for half in ("ar","nar"):
        (checkpoint/f"native-{half}.safetensors").write_bytes(b"isolated UI receipt fixture only")

def capture_pt():
    ui("open_settings", section="interface")
    options = wait_page(predicate=lambda page: 'select ' in page and '"pt"' in page)
    ui("select", ref=control(options, '"pt"'), value="pt")
    localized = wait_page(predicate=lambda page: 'value="pt"' in page)
    click('button "x icon"', localized)
    ui("navigate", view="adapters")
    click('tab "Treino"')
    click('Canções de exemplo')
    click('Resultado')
    page = wait_page('Para LoRA', predicate=lambda page: "50" in page)
    status = json.loads(request("/v1/training"))
    run = next(run for run in status["runs"] if run["id"] == "demo-run")
    assert run["status"] == "done", run
    assert any(row.get("step") == 50 if isinstance(row, dict) else row == 50 for row in run["checkpoints"]), run
    json_file(out/"pt-08-training-run.json", run)
    (out/"pt-08-training-page.txt").write_text(page, encoding="utf8")
    screenshot("pt-08-training")
    ui("open_settings", section="agent")
    page = wait_page("Agente (MCP)", "QA Agent command claude mcp add", "http://127.0.0.1:8791/mcp")
    (out/"pt-11-agent-page.txt").write_text(page, encoding="utf8")
    info = json.loads(request("/mcp/status"))
    assert info["window_open"], info
    json_file(out/"pt-11-agent-status.json", info)
    screenshot("pt-11-agent")

def native_editor_window(pid):
    from ctypes import wintypes

    windows = []
    user = ctypes.WinDLL("user32", use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.IsWindowVisible.argtypes = [wintypes.HWND]
    user.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]

    @callback_type
    def inspect(handle, _):
        owner = wintypes.DWORD()
        user.GetWindowThreadProcessId(handle, ctypes.byref(owner))
        if owner.value == pid and user.IsWindowVisible(handle):
            title = ctypes.create_unicode_buffer(256)
            user.GetWindowTextW(handle, title, len(title))
            if title.value.endswith(" - Audio Editor"):
                windows.append(title.value)
        return True

    user.EnumWindows(inspect, 0)
    return windows


def check_screens(root, pid):
    ui("navigate", view="create")
    page = wait_page(predicate=lambda value: '"Style"' in value or '"Use OpenRouter"' in value)
    if 'button "Use OpenRouter"' in page:
        click('button "Use OpenRouter"', page)
        page = wait_page('"Style"')
    ui("type", ref=control(page, '"Style"'), value="Hosted WebView acceptance")
    wait_page('value="Hosted WebView acceptance"')
    (out/"screen-create.txt").write_text(ui("read_page"), encoding="utf8")

    source = root/"hosted-music.wav"
    with wave.open(str(source), "wb") as audio:
        audio.setparams((1,2,24000,0,"NONE","not compressed"))
        frames = bytearray()
        for i in range(24000*30):
            amplitude = 8000 * (.55 + .45*math.sin(2*math.pi*2*i/24000))
            signal = amplitude * (.62*math.sin(2*math.pi*219*i/24000)+.38*math.sin(2*math.pi*437*i/24000))
            frames.extend(struct.pack("<h", int(signal)))
        audio.writeframes(frames)
    song = tool("library_import_audio", path=str(source), title="Hosted QA recording")
    assert song["id"], song
    playlist = tool("playlist_create", name="Hosted QA playlist", song_ids=[song["id"]])
    assert playlist["id"], playlist
    files = tool("library_song_files", song_id=song["id"])
    media = pathlib.Path(files["audio"]).parent
    for stem in ("vocals", "instrumental"):
        (media/f"{song['id']}-{stem}.wav").write_bytes(source.read_bytes())

    ui("navigate", view="library")
    page = wait_page("Hosted QA recording")
    (out/"screen-library.txt").write_text(page, encoding="utf8")
    click('Playlists')
    playlist_page = wait_page('Hosted QA playlist')
    row = next(line for line in playlist_page.splitlines() if 'Hosted QA playlist' in line and ' open' in line)
    ui("click", ref=re.search(r"(e\d+) open", row)[1])
    page = wait_page('Page: Hosted QA playlist', 'Hosted QA recording')
    (out/"screen-playlist.txt").write_text(page, encoding="utf8")

    tool("player_play", song_id=song["id"])
    tool("player_set", volume=.35, repeat="one")
    tool("player_seek", seconds=2)
    for _ in range(35):
        playback = tool("player_state")
        if playback["playing"] and playback["position_seconds"] > 2.1:
            break
        time.sleep(.2)
    else:
        raise AssertionError(("audio did not play", playback, ui("console")))
    json_file(out/"player.json", playback)
    eq = tool("equalizer_set", enabled=True, bands={"310": 3}, panel_open=True, preamp_db=-2)
    assert eq["enabled"] and eq["panel_open"] and eq["preamp_db"] == -2, eq
    assert any(row["hz"] == 310 and row["db"] == 3 for row in eq["bands"]), eq
    json_file(out/"equalizer.json", eq)

    for mode in ("milkdrop", "spectrum"):
        tool("visualizer_set", engine=mode, place="panel", locked=True, random=False)
        hashes = set()
        values = []
        for _ in range(35):
            page = ui("read_page")
            marker = next((line for line in page.splitlines() if line.startswith("QA visualizer ")), "")
            if marker:
                data = json.loads(marker[len("QA visualizer "):])
                if data["mode"] == mode and data["colored"] > 30:
                    hashes.add(data["hash"])
                    values.append(data)
                    if len(hashes) >= 2:
                        break
            time.sleep(.22)
        json_file(out/f"{mode}-frames.json", values)
        assert len(hashes) >= 2, (mode, values, page, ui("console"))
        screenshot(f"visualizer-{mode}")
    tool("visualizer_set", place="closed")
    tool("equalizer_set", panel_open=False)
    tool("winamp_set", on=True, windows={"equalizer":{"open":True},"playlist":{"open":True}})
    winamp = tool("winamp_get")
    assert winamp["on"] and winamp["windows"]["equalizer"]["open"] and winamp["windows"]["playlist"]["open"], winamp
    json_file(out/"winamp.json", winamp)
    screenshot("winamp")
    tool("winamp_set", on=False)
    tool("player_pause")

    abc = """X:1
T:
M:4/4
L:1/4
Q:1/4=120
V: Vocal clef=treble name="Vocal Melody" snm="Vocal"
V: Ins clef=treble name="Ins Melody" snm="Inst."
K:C
V: Vocal
C D E F |
V: Ins
G4 |
"""
    notes = tool("score_to_midi", abc=abc)
    assert notes.get("data"), notes
    midi = root/"hosted-midi.mid"
    midi.write_bytes(base64.b64decode(notes["data"]))
    assert midi.read_bytes().startswith(b"MThd")
    tool("midi_set", song_id=song["id"], path=str(midi))
    midi_result = tool("midi_get", song_id=song["id"], response_format="detailed")
    json_file(out/"audio-to-midi.json", midi_result)
    assert "notes" in midi_result and midi_result["notes"], midi_result

    ui("navigate", view="tools")
    click('button "Hosted QA recording"')
    page = wait_page('Open the MIDI editor', 'Separate again', 'Open the track')
    (out/"screen-tools.txt").write_text(page, encoding="utf8")
    click('Open the MIDI editor', page)
    page = wait_page('Page: MIDI editor', 'button "Close"')
    (out/"screen-midi-editor.txt").write_text(page, encoding="utf8")
    click('button "Close"')
    stem_status = tool("stems_get", song_id=song["id"])
    json_file(out/"stems.json", stem_status)
    assert {"vocals","instrumental"} <= set(stem_status["stems"]), stem_status

    click('Process audio')
    process_page = wait_page('Peak normalisation', 'button "Process"')
    click('Peak normalisation', process_page)
    click('button "Process"')
    (out/"screen-processing.txt").write_text(wait_page('Save as a new track'), encoding="utf8")
    click('Save as a new track')
    for _ in range(40):
        library = json.loads(request('/v1/library/songs'))
        if any(entry.get('metadata', {}).get('derived', {}).get('from') == song['id']
               for entry in library):
            json_file(out/'processing-library.json', library)
            break
        time.sleep(.25)
    else:
        raise AssertionError(("Processed track is absent", library, ui("read_page")))

    click('Open the track')
    for _ in range(30):
        if windows := native_editor_window(pid):
            json_file(out/'audio-editor-window.json', dict(pid=pid, titles=windows))
            break
        time.sleep(.25)
    else:
        raise AssertionError(('Audio Editor window did not open', ui('console')))

    ui("navigate", view="adapters")
    click('tab "Catalog"')
    (out/"screen-lora-catalog.txt").write_text(wait_page('Search the catalog'), encoding="utf8")
    json_file(out/'lora-catalog.json', tool('lora_list', response_format='detailed'))
    click('tab "Training"')
    click('Canções de exemplo')
    click('Result')
    (out/"screen-training.txt").write_text(wait_page('To LoRA', '50'), encoding="utf8")
    ui("navigate", view="news")
    (out/"screen-news.txt").write_text(wait_page('Page: News'), encoding="utf8")
    sections = dict(account="Account", models="Models", engine="Local engine", cloud="Providers",
                    agent="Agent (MCP)", covers="Cover art", interface="Appearance", privacy="Privacy", about="About")
    for section, label in sections.items():
        ui("open_settings", section=section)
        page = wait_page("Page: " + label)
        (out/f"settings-{section}.txt").write_text(page, encoding="utf8")
    (out/"console.txt").write_text(ui("console"), encoding="utf8")
    assert not re.search(r"butterchurn.*(?:error|not a function)|(?:error|not a function).*butterchurn", ui("console"), re.I)


with tempfile.TemporaryDirectory(prefix="studio-hosted-webview-") as directory:
    root = pathlib.Path(directory)
    seed_training(root)
    for folder in ("user", "roaming", "local"):
        (root/folder).mkdir()
    env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(root), YUE_STUDIO_PORT="8791",
        HOME=str(root/"user"), USERPROFILE=str(root/"user"), APPDATA=str(root/"roaming"),
        LOCALAPPDATA=str(root/"local"), DO_NOT_TRACK="1",
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--autoplay-policy=no-user-gesture-required --use-angle=swiftshader --enable-unsafe-swiftshader")
    with (out/"service.log").open("wb") as log:
        process = subprocess.Popen([str(args.exe.resolve(strict=True))], env=env, stdout=log, stderr=log)
        try:
            for _ in range(150):
                assert process.poll() is None, process.returncode
                try:
                    request("/health")
                    break
                except OSError:
                    time.sleep(.25)
            else:
                raise TimeoutError("Studio health did not become ready")
            rpc("initialize", dict(protocolVersion="2025-03-26", capabilities={},
                clientInfo=dict(name="studio-hosted-webview", version="1")))
            rpc("notifications/initialized", {}, notify=True)
            wait_page(predicate=lambda page: "button" in page)
            capture_pt() if args.suite == "pt" else check_screens(root, process.pid)
        finally:
            json_file(out/"trace.json", trace)
            stop = subprocess.Popen([str(args.exe.resolve()), "--stop"], env=env)
            stop.wait(timeout=10)
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
