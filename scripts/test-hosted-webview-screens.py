"""Drive only the bundled Studio WebView on the isolated Windows runner."""
import argparse, base64, json, os, pathlib, struct, subprocess, tempfile, time, urllib.request, wave, math

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

def tool(name, **fields):
    result = rpc("tools/call", dict(name=name, arguments=fields))
    assert not result.get("isError"), (name, fields, result)
    content = result.get("content", [])
    if name == "ui_screenshot":
        return next(item for item in content if item.get("type") == "image")["data"]
    answer = result.get("structuredContent") or content[0]["text"]
    trace.append(dict(tool=name, fields=fields, answer=answer))
    return answer

def ui(name, **fields):
    return tool("ui_" + name, **fields)

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
    wait_page('Língua', predicate=lambda page: 'value="pt"' in page)
    ui("press_key", key="Escape")
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
    page = wait_page("Agente (MCP)", "Endereço", "Claude Code")
    (out/"pt-11-agent-page.txt").write_text(page, encoding="utf8")
    info = json.loads(request("/mcp/status"))
    assert info["window_open"], info
    json_file(out/"pt-11-agent-status.json", info)
    screenshot("pt-11-agent")

def check_screens(root):
    for view in ("create", "library", "search", "tools", "adapters", "news"):
        ui("navigate", view=view)
        wait_page(predicate=lambda page: "Page:" in page or "button" in page)
        (out/f"screen-{view}.txt").write_text(ui("read_page"), encoding="utf8")
    for section in ("account", "models", "engine", "cloud", "agent", "covers", "interface", "privacy", "about"):
        ui("open_settings", section=section)
        page = wait_page(predicate=lambda page: "Settings" in page)
        (out/f"settings-{section}.txt").write_text(page, encoding="utf8")
    (out/"console.txt").write_text(ui("console"), encoding="utf8")

with tempfile.TemporaryDirectory(prefix="studio-hosted-webview-") as directory:
    root = pathlib.Path(directory)
    seed_training(root)
    for folder in ("user", "roaming", "local"):
        (root/folder).mkdir()
    env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(root), YUE_STUDIO_PORT="8791",
        HOME=str(root/"user"), USERPROFILE=str(root/"user"), APPDATA=str(root/"roaming"),
        LOCALAPPDATA=str(root/"local"), DO_NOT_TRACK="1")
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
            capture_pt() if args.suite == "pt" else check_screens(root)
        finally:
            json_file(out/"trace.json", trace)
            stop = subprocess.Popen([str(args.exe.resolve()), "--stop"], env=env)
            stop.wait(timeout=10)
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
