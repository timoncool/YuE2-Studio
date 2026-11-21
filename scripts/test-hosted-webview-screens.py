"""Drive only the bundled Studio WebView on the isolated Windows runner."""
import argparse, base64, ctypes, hashlib, json, os, pathlib, re, shutil, struct, subprocess, tempfile, time, urllib.request, wave, math, zlib
from contextlib import contextmanager

p = argparse.ArgumentParser()
p.add_argument("exe", type=pathlib.Path)
p.add_argument("output", type=pathlib.Path)
p.add_argument("--suite", choices=["screens", "lrc", "print", "eqf", "video", "dj"], default="screens")
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


def save_dialog_file(pid, path, output="save-dialog.json", action="save"):
    # UI Automation operates only on the isolated runner's native Save As dialog.
    script = r"""
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
$root = [Windows.Automation.AutomationElement]::RootElement
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class StudioQaWindows {
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr window, uint command);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetProcessWindowStation();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern IntPtr GetThreadDesktop(uint threadId);
  [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll")] public static extern bool CloseDesktop(IntPtr desktop);
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern bool GetUserObjectInformation(IntPtr handle, int index, StringBuilder text, int bytes, out int needed);
  public static string ObjectName(IntPtr handle) {
    var text = new StringBuilder(256);
    int needed;
    return handle == IntPtr.Zero || !GetUserObjectInformation(handle, 2, text, text.Capacity * 2, out needed)
      ? null : text.ToString();
  }
}
'@
$seen = @{}
$windowThreads = @{}
$desktop = [StudioQaWindows]::OpenInputDesktop(0,$false,1)
$windowStation = [StudioQaWindows]::ObjectName([StudioQaWindows]::GetProcessWindowStation())
$inputDesktop = [StudioQaWindows]::ObjectName($desktop)
if ($desktop -ne [IntPtr]::Zero) { [void][StudioQaWindows]::CloseDesktop($desktop) }
$runner = Get-Process -Id $PID
$studio = Get-Process -Id ([int]$env:STUDIO_QA_PID)
$station = @{station=$windowStation; desktop=$inputDesktop; runner=$runner.SessionId; studio=$studio.SessionId
             explorer=@(Get-Process -Name explorer -ErrorAction SilentlyContinue | Select-Object Id,SessionId)
             studioPid=$studio.Id}
$station | ConvertTo-Json -Compress
if (!$inputDesktop -or $windowStation -ne 'WinSta0' -or $runner.SessionId -ne $studio.SessionId) {
   throw 'The studio and the locator must share the interactive Windows desktop'
}
$deadline = [DateTime]::UtcNow.AddSeconds(20)
do {
  $windows = $root.FindAll([Windows.Automation.TreeScope]::Children,
    [Windows.Automation.Condition]::TrueCondition)
  foreach ($dialog in $windows) {
    $current = $dialog.Current
    if ($current.ProcessId -ne [int]$env:STUDIO_QA_PID) {
      if ($current.Name -ne 'Save As') { continue }
      $parent = [StudioQaWindows]::GetWindow([IntPtr]$current.NativeWindowHandle, 4)
      $windowOwner = [uint32]0
      [void][StudioQaWindows]::GetWindowThreadProcessId($parent, [ref]$windowOwner)
      if ($windowOwner -ne [int]$env:STUDIO_QA_PID) { continue }
    }
    $key = "$($current.ProcessId) $($current.ClassName) $($current.Name)"
    if (!$windowThreads.ContainsKey($key) -and $current.NativeWindowHandle) {
      $windowPid = [uint32]0
      $thread = [StudioQaWindows]::GetWindowThreadProcessId([IntPtr]$current.NativeWindowHandle,[ref]$windowPid)
      $windowThreads[$key] = @{
        hwnd=$current.NativeWindowHandle
        thread=$thread
        desktop=[StudioQaWindows]::ObjectName([StudioQaWindows]::GetThreadDesktop($thread))
        ownerHwnd=[StudioQaWindows]::GetWindow([IntPtr]$current.NativeWindowHandle,4).ToInt64()
        initiallyForeground=[StudioQaWindows]::GetForegroundWindow().ToInt64() -eq $current.NativeWindowHandle
        bounds=@{x=$current.BoundingRectangle.X; y=$current.BoundingRectangle.Y
                 width=$current.BoundingRectangle.Width; height=$current.BoundingRectangle.Height}
      }
    }
    if ($current.ClassName -ne '#32770' -and $current.Name -notlike '*Save As*') {
      $seen[$key] = 'window'
      continue
    }
    $elements = $dialog.FindAll([Windows.Automation.TreeScope]::Descendants,
      [Windows.Automation.Condition]::TrueCondition)
    $seen[$key] = @($elements | Where-Object {
      $_.Current.ControlType -in @([Windows.Automation.ControlType]::Edit, [Windows.Automation.ControlType]::Button)
    } | ForEach-Object {
      "$($_.Current.ControlType.ProgrammaticName) id=$($_.Current.AutomationId) name=$($_.Current.Name)"
    }) -join ', '
    $fields = @($elements | Where-Object {
      $_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and
      ($_.Current.AutomationId -in @('1001','1148') -or $_.Current.Name -match '^File name')
    })
    $save = @($elements | Where-Object {
      $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and
      ($_.Current.AutomationId -eq '1' -or $_.Current.Name -eq 'Save')
    })
    $name = $dialog.Current.Name
    if ($env:STUDIO_QA_DIALOG_ACTION -eq 'save' -and $fields -and $save) {
      $fields[0].GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($env:STUDIO_QA_FILE)
      $save[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
      @{name=$name; method='UIA'; windows=$windowThreads} | ConvertTo-Json -Depth 5 -Compress
      exit 0
    }
    $hwnd = [IntPtr]$current.NativeWindowHandle
    if ($current.ClassName -ne '#32770' -or $name -ne 'Save As' -or $hwnd -eq [IntPtr]::Zero) { continue }
    [void][StudioQaWindows]::SetForegroundWindow($hwnd)
    if ([StudioQaWindows]::GetForegroundWindow() -ne $hwnd) { continue }
    $bounds = $current.BoundingRectangle
    $monitor = [Windows.Forms.Screen]::FromHandle($hwnd).Bounds
    if ($bounds.IsEmpty -or $bounds.Right -le $monitor.Left -or $bounds.Left -ge $monitor.Right -or
        $bounds.Bottom -le $monitor.Top -or $bounds.Top -ge $monitor.Bottom) { throw 'Save As is off-screen' }
    if ($env:STUDIO_QA_DIALOG_ACTION -eq 'cancel') {
      [Windows.Forms.SendKeys]::SendWait('{ESC}')
    } else {
      [Windows.Forms.SendKeys]::SendWait('%n')
      [Windows.Forms.SendKeys]::SendWait('^a')
      $keys = [regex]::Replace($env:STUDIO_QA_FILE, '[+^%~(){}\[\]]', { param($match) '{' + $match.Value + '}' })
      [Windows.Forms.SendKeys]::SendWait($keys)
      [Windows.Forms.SendKeys]::SendWait('{ENTER}')
    }
    @{name=$name; method='verified foreground dialog'; action=$env:STUDIO_QA_DIALOG_ACTION
      monitor=@{x=$monitor.X; y=$monitor.Y; width=$monitor.Width; height=$monitor.Height}; windows=$windowThreads} | ConvertTo-Json -Depth 5 -Compress
    exit 0
  }
  Start-Sleep -Milliseconds 200
} while ([DateTime]::UtcNow -lt $deadline)
@{controls=$seen; windows=$windowThreads} | ConvertTo-Json -Depth 5 -Compress
throw 'The isolated Save As dialog did not expose a file name and Save button'
"""
    encoded = base64.b64encode(script.encode("utf-16-le")).decode("ascii")
    run = subprocess.run(["powershell.exe", "-NoProfile", "-STA", "-EncodedCommand", encoded],
                         env=dict(os.environ, STUDIO_QA_PID=str(pid), STUDIO_QA_FILE=str(path),
                                  STUDIO_QA_DIALOG_ACTION=action),
                         capture_output=True, text=True, timeout=27)
    json_file(out/output, dict(code=run.returncode, stdout=run.stdout, stderr=run.stderr))
    assert run.returncode == 0, (run.stdout, run.stderr)


def verify_lrc_file(destination, expected, song_id, route, receipt="lrc-file.json"):
    for _ in range(40):
        if destination.is_file() and destination.stat().st_size == len(expected.encode("utf8")):
            saved = destination.read_bytes()
            break
        time.sleep(.25)
    else:
        raise AssertionError(("LRC file was not saved", ui("console")))
    assert saved.decode("utf-8", errors="strict") == expected
    assert re.findall(r"^\[(\d\d:\d\d\.\d\d)\]", expected, re.M) == ["00:00.50", "00:01.75"]
    (out/destination.name).write_bytes(saved)
    json_file(out/receipt, dict(song_id=song_id, bytes=len(saved), sha256=hashlib.sha256(saved).hexdigest(),
                                       timestamps=["00:00.50", "00:01.75"], name=destination.name, route=route))


def check_lrc(root, pid, media=None):
    title = "Hosted LRC receipt"
    source = root/"source.wav"
    with wave.open(str(source), "wb") as audio:
        audio.setparams((1, 2, 24000, 0, "NONE", "not compressed"))
        audio.writeframes(bytes(24000*8))
    song = tool("library_import_audio", path=str(source), title=title)
    abc = ('X:1\nT:\nM:4/4\nL:1/4\nQ:1/4=120\n'
           'V: Vocal clef=treble name="Vocal Melody" snm="Vocal"\n'
           'V: Ins clef=treble name="Ins Melody" snm="Inst."\n'
           'K:C\nV: Vocal\nC D E F |\nV: Ins\nG4 |')
    record = tool("library_song_get", song_id=song["id"], response_format="detailed")
    if media == "print":
        abc = 'X:1\nT:Print fixture\nM:4/4\nL:1/4\nK:C\n"Cmaj7"C D E F|\nw:Blue birds greet dawn\n"G7"G A B c|\nw:Bright sails head home'
        record["lyrics"] = "Blue birds greet dawn\nBright sails head home"
    record["generation_settings"] = {**(record.get("generation_settings") or {}), "abc": abc}
    tool("library_song_update", song_id=song["id"], song=record)
    expected = "[00:00.50]Привет, мир\n[00:01.75]Canção em português\n"
    data = tool("karaoke_set", song_id=song["id"], lrc=expected)
    assert data == {"lrc": expected, "lines": 2}, data
    ui("navigate", view="library")
    context = f"song {song['id']}:"
    page = wait_page(predicate=lambda value: any(context in line and "Song Details" in line for line in value.splitlines()))
    row = next(line for line in page.splitlines() if context in line)
    ui("click", ref=re.search(r"(e\d+) Song Details", row)[1])
    page = wait_page('button "Download LRC (Lyrics)"')
    (out/"lrc-details.txt").write_text(page, encoding="utf8")
    destination = root/f"{title}.lrc"
    script = pathlib.Path(__file__).with_name("observe-hosted-lrc.mjs")
    observer = subprocess.Popen(["node", str(script), str(out), str(root)],
                                stdin=subprocess.PIPE, cwd=out)
    try:
        for _ in range(40):
            if (out/"lrc-observer-ready").is_file():
                break
            assert observer.poll() is None, "The WebView observer exited"
            time.sleep(.25)
        else:
            raise TimeoutError("The WebView observer did not initialize")
        def failed_save(label, kind, extension):
            name = f"conflict-{kind}.{extension}"
            marker = out/"save-conflict.json"
            json_file(out/"save-conflict.tmp", dict(file=name))
            (out/"save-conflict.tmp").replace(marker)
            click(label)
            save_dialog_file(pid, root/name, f"{kind}-conflict-dialog.json")
            for _ in range(60):
                try:
                    events = json.loads((out/"lrc-webview-network.json").read_text(encoding="utf8"))
                except (OSError, json.JSONDecodeError):
                    time.sleep(.2)
                    continue
                failures = [entry for entry in events if entry.get("visibleSaveError")
                            and entry["id"] not in seen_errors]
                if failures:
                    assert (root/name).is_dir() and not marker.exists()
                    seen_errors.add(failures[0]["id"])
                    json_file(out/f"{kind}-visible-error.json", failures[0])
                    return
                time.sleep(.2)
            raise AssertionError(f"No visible write error for {name}")
        seen_errors = set()
        def cancel_export(label, kind, after_label=None):
            def cancelled_replies():
                try:
                    events = json.loads((out/"lrc-webview-network.json").read_text(encoding="utf8"))
                except (OSError, json.JSONDecodeError):
                    return None
                return {entry["id"] for entry in events if entry.get("responseBody")
                        and json.loads(entry["responseBody"]).get("cancelled") is True}
            before = cancelled_replies()
            assert before is not None
            click(label)
            target = root/f"Отказ-{kind}"
            save_dialog_file(pid, target, f"{kind}-cancel-dialog.json", action="cancel")
            wait_page(after_label or label)
            for _ in range(40):
                after = cancelled_replies()
                if after and after - before:
                    break
                time.sleep(.2)
            else:
                raise AssertionError(f"{kind}: service did not acknowledge cancellation")
            assert not list(root.glob(target.name + "*"))
            json_file(out/f"{kind}-cancel.json", dict(response_ids=sorted(after-before), file_created=False))
        if media == "print":
            check_songbook_print(root, pid)
            return
        if media:
            click('button "x icon"')
            if media == "eqf":
                check_eqf(root, pid, cancel_export, failed_save)
            else:
                check_media_exports(root, pid, media, cancel_export, failed_save)
            return
        click('button "Download LRC (Lyrics)"', page)
        save_dialog_file(pid, destination)
        verify_lrc_file(destination, expected, song["id"], "desktop")
        click('button "Download LRC (Lyrics)"')
        cancelled = root/"Cancelled LRC.lrc"
        save_dialog_file(pid, cancelled, "cancel-dialog.json", action="cancel")
        for _ in range(30):
            try:
                entries = json.loads((out/"lrc-webview-network.json").read_text(encoding="utf8"))
            except (OSError, json.JSONDecodeError):
                entries = []
            replies = [json.loads(entry["responseBody"]) for entry in entries if entry.get("responseBody")]
            if any(reply.get("cancelled") is True for reply in replies):
                break
            time.sleep(.2)
        else:
            raise AssertionError("Save As did not acknowledge cancellation")
        assert not cancelled.exists()
        writes = [entry["request"]["url"] for entry in entries if
                  entry.get("request", {}).get("method") == "POST" and "/v1/files/save/" in entry["request"]["url"]]
        assert len(writes) == 1, writes
        json_file(out/"cancel-result.json", {"cancelled": True, "file_created": False, "write_requests": len(writes)})
        click('button "Download LRC (Lyrics)"')
        repeated = root/"Repeated LRC.lrc"
        save_dialog_file(pid, repeated, "repeat-dialog.json")
        verify_lrc_file(repeated, expected, song["id"], "desktop", "repeat-lrc-file.json")

        failed_save('button "Download LRC (Lyrics)"', "lrc", "lrc")
        for label, extension in [("Download Audio", "wav"), ("Download SRT (subtitles)", "srt")]:
            control_name = f'button "{label}"'
            destination = root / f"Сохранённый дубль.{extension}"
            click(control_name)
            save_dialog_file(pid, destination, f"{extension}-dialog.json")
            for _ in range(60):
                if destination.is_file() and destination.stat().st_size:
                    break
                time.sleep(.2)
            saved = destination.read_bytes()
            if extension == "wav":
                # Updating the score embeds metadata in the library WAV. Export retains
                # those bytes, while the imported fixture still defines the expected PCM.
                assert saved == request(f"/v1/library/media/{song['id']}"), "Export differs from current library audio"
                with wave.open(str(source), "rb") as original, wave.open(str(destination), "rb") as exported:
                    assert exported.getparams() == original.getparams()
                    assert exported.readframes(exported.getnframes()) == original.readframes(original.getnframes())
            else:
                subtitles = saved.decode("utf-8")
                assert "00:00:00,500 -->" in subtitles and "00:00:01,750 -->" in subtitles, subtitles
                assert "Привет, мир" in subtitles and "Canção em português" in subtitles, subtitles
            click(control_name)
            cancelled = root / f"Отмена.{extension}"
            save_dialog_file(pid, cancelled, f"{extension}-cancel-dialog.json", action="cancel")
            wait_page(control_name)
            assert not cancelled.exists(), cancelled
            failed_save(control_name, f"download-{extension}", extension)
            json_file(out/f"{extension}-export.json", dict(song_id=song["id"], bytes=len(saved),
                      sha256=hashlib.sha256(saved).hexdigest(), cancelled_file_absent=True))
        score_button = 'button ".abc"'
        click(score_button)
        score_file = root/"Ноты.abc"
        save_dialog_file(pid, score_file, "abc-dialog.json")
        for _ in range(50):
            if score_file.is_file() and score_file.read_bytes() == (abc + "\n").encode():
                break
            time.sleep(.2)
        else:
            raise AssertionError("Saved ABC score differs from song")
        click(score_button)
        save_dialog_file(pid, root/"Cancel.abc", "abc-cancel-dialog.json", action="cancel")
        failed_save(score_button, "score", "abc")
        for label, extension in (("PDF", "pdf"), ("MIDI", "mid")):
            button = f'button "{label}"'
            output = root/f"Партитура.{extension}"
            click(button)
            save_dialog_file(pid, output, f"score-{extension}-dialog.json")
            for _ in range(60):
                content = output.read_bytes() if output.is_file() else b""
                if extension == "pdf":
                    complete = content.startswith(b"%PDF-") and content.rstrip().endswith(b"%%EOF")
                else:
                    complete = content.startswith(b"MThd") and b"MTrk" in content and b"\xff\x2f\x00" in content
                if complete:
                    break
                time.sleep(.2)
            else:
                raise AssertionError(f"Invalid saved score {extension}: {len(content)} bytes")
            cancel_path = root/f"Отмена-партитуры.{extension}"
            click(button)
            save_dialog_file(pid, cancel_path, f"score-{extension}-cancel-dialog.json", action="cancel")
            wait_page(button)
            assert not cancel_path.exists()
            failed_save(button, f"score-{extension}", extension)
            json_file(out/f"score-{extension}-export.json", dict(bytes=len(content),
                      sha256=hashlib.sha256(content).hexdigest(), cancelled_file_absent=True))
        json_file(out/"score.json", dict(bytes=score_file.stat().st_size, sha256=hashlib.sha256(score_file.read_bytes()).hexdigest()))
        click('button "x icon"')
        page = wait_page(predicate=lambda text: context in text and 'button "Download LRC (Lyrics)"' not in text)
        row = next(line for line in page.splitlines() if context in line)
        ui("click", ref=re.search(r"(e\d+) More actions", row)[1])
        page = wait_page(predicate=lambda value: any(context in line and " Export…" in line for line in value.splitlines()))
        row = next(line for line in page.splitlines() if context in line)
        ui("click", ref=re.search(r"(e\d+) Export…(?:,|$)", row)[1])
        for fmt in ("wav", "flac", "mp3"):
            wait_page('button "Export"')
            ui("select", text="Format", value=fmt)
            click('button "Export"')
            click('button "Save…"')
            target = root/f"Мастер.{fmt}"
            save_dialog_file(pid, target, f"master-{fmt}-dialog.json")
            for _ in range(80):
                if target.exists() and target.stat().st_size > 128:
                    break
                time.sleep(.2)
            blob = target.read_bytes()
            assert (blob[:4] == b"RIFF" and blob[8:12] == b"WAVE" if fmt == "wav" else
                    blob[:4] == b"fLaC" if fmt == "flac" else
                    blob.startswith(b"ID3") or blob[0] == 255 and blob[1] & 224 == 224), (fmt, blob[:12])
            json_file(out/f"master-{fmt}.json", dict(bytes=len(blob), sha256=hashlib.sha256(blob).hexdigest()))
            cancel_export('button "Save…"', f"master-{fmt}")
            failed_save('button "Save…"', f"master-{fmt}", fmt)
            click('button "Export again"')
        import zipfile
        for kind in ("track", "ableton"):
            if kind == "ableton":
                click('button "Make the project"')
            label = 'button "Save…"' if kind == "ableton" else 'button "Export… ZIP"'
            click(label)
            target = root/f"Проект-{kind}.zip"
            save_dialog_file(pid, target, f"{kind}-dialog.json")
            for _ in range(80):
                if zipfile.is_zipfile(target):
                    break
                time.sleep(.2)
            with zipfile.ZipFile(target) as archive:
                assert archive.testzip() is None
                members = archive.namelist()
                assert any(name.lower().endswith(".wav") for name in members), members
                if kind == "ableton":
                    assert any(name.endswith(".als") for name in members), members
                json_file(out/f"{kind}-archive.json", dict(members=members, bytes=target.stat().st_size))
            cancel_export(label, kind)
            failed_save(label, kind, "zip")
    finally:
        observer.stdin.close()
        try:
            observer.wait(timeout=4)
        except subprocess.TimeoutExpired:
            observer.terminate()
            observer.wait(timeout=4)


def check_songbook_print(root, pid):
    click('button "Songbook"')
    wait_page('button "Print A4"')
    file_port = os.environ.get("STUDIO_QA_FILE_PORT")
    pdf = pathlib.Path(file_port) if file_port else root/"songbook.pdf"
    assert not pdf.exists(), "The print file port already contains an earlier document"
    # WebView2 may show its own print preview even with a default printer.
    # Read the isolated runner's UIA tree and confirm the visible Print action.
    print_dialog = r"""
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
$tree = [Windows.Automation.TreeScope]
$all = [Windows.Automation.Condition]::TrueCondition
$root = [Windows.Automation.AutomationElement]::RootElement
$confirmed = $false
$settingsOpen = $false
$paperOpen = $false
$paperSet = $false
$deadline = [DateTime]::UtcNow.AddSeconds(28)
while ([DateTime]::UtcNow -lt $deadline -and !(Test-Path -LiteralPath $env:STUDIO_QA_PDF)) {
  $ids = [Collections.Generic.HashSet[int]]::new()
  [void]$ids.Add([int]$env:STUDIO_QA_PID)
  $procs = @(Get-CimInstance Win32_Process)
  do {
    $count = $ids.Count
    foreach ($p in $procs) {
      if ($ids.Contains([int]$p.ParentProcessId)) { [void]$ids.Add([int]$p.ProcessId) }
    }
  } while ($count -ne $ids.Count)
  foreach ($window in $root.FindAll($tree::Children,$all)) {
    if (!$ids.Contains($window.Current.ProcessId) -and $window.Current.Name -notmatch '^(Print(ing)?|Save As|Save Print Output As)$') { continue }
    $elements = $window.FindAll($tree::Descendants,$all)
    $buttons = @($elements | Where-Object {
      $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.IsEnabled
    })
    $confirm = $buttons | Where-Object {$_.Current.Name -eq 'Print' -and !$_.Current.IsOffscreen} | Select-Object -First 1
    if ($confirmed) {
      $name = $elements | Where-Object {
        $_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and
        ($_.Current.AutomationId -in @('1001','1148') -or $_.Current.Name -match '^File name')
      } | Select-Object -First 1
      $save = $buttons | Where-Object {
        $_.Current.AutomationId -eq '1' -or $_.Current.Name -match '^&?Save$'
      } | Select-Object -First 1
      Write-Output (@{
        window=$window.Current.Name;fileName=([bool]$name);save=([bool]$save)
        buttons=@($buttons | ForEach-Object {"$($_.Current.AutomationId):$($_.Current.Name)"})
      } | ConvertTo-Json -Compress)
      if ($name) {
        $name.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($env:STUDIO_QA_PDF)
        if ($save) {
          $save.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
        } else {
          $name.SetFocus()
          [Windows.Forms.SendKeys]::SendWait('{ENTER}')
        }
        Write-Output 'Selected PDF output path'
        exit 0
      }
      $printOutputFolder = @($buttons | Where-Object {$_.Current.Name -eq 'New folder'}).Count -eq 1 -and
        @($buttons | Where-Object {$_.Current.AutomationId -eq 'HelpButton'}).Count -eq 1
      if ($printOutputFolder) {
        # The shell exposes its folder view but can omit the lower controls
        # from UIA when the output chooser is embedded in the Studio owner.
        $focus = [Windows.Automation.AutomationElement]::FocusedElement
        if (!$focus -or !$ids.Contains($focus.Current.ProcessId)) { throw 'Print output chooser is not focused' }
        [Windows.Forms.SendKeys]::SendWait('%n')
        [Windows.Forms.SendKeys]::SendWait('^a')
        [Windows.Forms.SendKeys]::SendWait($env:STUDIO_QA_PDF)
        [Windows.Forms.SendKeys]::SendWait('{ENTER}')
        Write-Output 'Selected PDF output path in the focused Windows folder chooser'
        exit 0
      }
    }
    if ($confirm -and !$confirmed) {
      $destinations = @($elements | Where-Object {
        $_.Current.Name -match 'Printer|Destination|Studio QA PDF|Microsoft Print' -and !$_.Current.IsOffscreen
      } | ForEach-Object { $_.Current.Name })
      Write-Output (@{destinationControls=$destinations} | ConvertTo-Json -Compress)
      if ($env:STUDIO_QA_PRINT_ACTION -eq 'cancel') {
        $cancel = $buttons | Where-Object {$_.Current.Name -eq 'Cancel'} | Select-Object -First 1
        if (!$cancel) { throw 'The print preview has no Cancel button' }
        $cancel.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
        Write-Output 'Cancelled print preview'
        exit 0
      }
      if (!$paperSet -and $env:STUDIO_QA_FILE_PORT) {
        if (!$settingsOpen) {
          $more = $buttons | Where-Object {$_.Current.Name -eq 'More settings'} | Select-Object -First 1
          if (!$more) { throw 'Print preview does not show More settings' }
          $expand = $null
          if ($more.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$expand)) {
            $expand.Expand()
          } else {
            $more.SetFocus()
            [Windows.Forms.SendKeys]::SendWait(' ')
          }
          $settingsOpen = $true
          break
        }
        $paper = $elements | Where-Object {
          $_.Current.ControlType -eq [Windows.Automation.ControlType]::ComboBox -and $_.Current.Name -match 'Paper size'
        } | Select-Object -First 1
        if (!$paper) {
          $elements | Where-Object {$_.Current.Name -match 'Paper|Size|Letter|A4'} | ForEach-Object {
            Write-Output "$($_.Current.ControlType.ProgrammaticName):$($_.Current.Name)"
          }
          continue
        }
        if (!$paperOpen) {
          $pattern = $null
          if ($paper.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$pattern)) {
            $pattern.Expand()
          } else {
            $paper.SetFocus()
            [Windows.Forms.SendKeys]::SendWait('%{DOWN}')
          }
          $paperOpen = $true
          break
        }
        $a4 = $elements | Where-Object {
          $_.Current.Name -match '(^|\s)A4($|[\s(])' -and !$_.Current.IsOffscreen
        } | Select-Object -First 1
        if (!$a4) { continue }
        $choice = $null
        if ($a4.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern,[ref]$choice)) {
          $choice.Select()
        } elseif ($a4.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$choice)) {
          $choice.Invoke()
        } else {
          $a4.SetFocus()
          [Windows.Forms.SendKeys]::SendWait('{ENTER}')
        }
        $paperSet = $true
        Write-Output 'Selected A4 in the WebView print preview'
        break
      }
      $confirm.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
      Write-Output 'Confirmed print'
      $confirmed = $true
    }
  }
  Start-Sleep -Milliseconds 500
}
"""
    (out/"click-print").touch()
    cancelled = root/"songbook-cancelled.pdf"
    result = subprocess.run(["powershell", "-NoProfile", "-Command", print_dialog],
                            env=dict(os.environ, STUDIO_QA_PDF=str(cancelled), STUDIO_QA_PID=str(pid),
                                     STUDIO_QA_PRINT_ACTION="cancel"),
                            capture_output=True, text=True, timeout=40, check=True)
    (out/"print-cancel-dialog.txt").write_text(result.stdout + result.stderr, encoding="utf8")
    assert "Cancelled print preview" in result.stdout and not cancelled.exists()
    json_file(out/"songbook-print-cancel.json", dict(cancelled=True, file_created=False))
    (out/"click-print").touch()
    with (out/"print-dialog.txt").open("w", encoding="utf8") as log:
        subprocess.run(["powershell", "-NoProfile", "-Command", print_dialog],
                       env=dict(os.environ, STUDIO_QA_PDF=str(pdf), STUDIO_QA_PID=str(pid), STUDIO_QA_PRINT_ACTION="save"),
                       stdout=log, stderr=subprocess.STDOUT, timeout=40, check=True)
    for _ in range(150):
        try:
            printed = pdf.read_bytes()
        except OSError:
            printed = b""
        if printed.startswith(b"%PDF-") and printed.rstrip().endswith(b"%%EOF"):
            break
        time.sleep(.2)
    else:
        raise AssertionError("Windows PDF printer did not complete the songbook")
    (out/"songbook.pdf").write_bytes(printed)
    from pypdf import PdfReader
    pages = PdfReader(out/"songbook.pdf").pages
    boxes = [[float(page.mediabox.width), float(page.mediabox.height)] for page in pages]
    json_file(out/"songbook-print.json", dict(bytes=len(printed), sha256=hashlib.sha256(printed).hexdigest(), media_boxes=boxes))
    assert all(abs(width - 595) < 2 and abs(height - 842) < 2 for width, height in boxes), boxes
    click('button "Close songbook"')


def check_eqf(root, pid, cancel_export, failed_save):
    tool("equalizer_set", enabled=True, bands={"310":3}, panel_open=True, preamp_db=-2)
    label = 'button "Export .EQF"'
    click(label)
    target = root/"Профиль.eqf"
    save_dialog_file(pid, target, "eqf-dialog.json")
    for _ in range(50):
        if target.is_file() and target.read_bytes().startswith(b"Winamp EQ library file"):
            break
        time.sleep(.2)
    else:
        raise AssertionError("Missing EQF preset header")
    cancel_export(label, "eqf")
    failed_save(label, "equalizer", "eqf")
    json_file(out/"eqf.json", dict(bytes=target.stat().st_size, sha256=hashlib.sha256(target.read_bytes()).hexdigest()))


def check_media_exports(root, pid, media, cancel_export, failed_save):
    title = "Hosted export tone"
    source = root/"tone.wav"
    with wave.open(str(source), "wb") as wav:
        wav.setparams((1, 2, 24000, 0, "NONE", "not compressed"))
        wav.writeframes(b"".join(
            struct.pack("<h", round(12000*math.sin(2*math.pi*440*index/24000)))
            for index in range(24000*(2 if media == "video" else 6))
        ))
    song = tool("library_import_audio", path=str(source), title=title)
    stored = pathlib.Path(tool("library_song_files", song_id=song["id"])["audio"])
    original = stored.read_bytes()

    def saved(path):
        for _ in range(80):
            if path.is_file() and path.stat().st_size > 256:
                return
            time.sleep(.2)
        raise AssertionError(f"No output at {path.name}")

    def digest(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def probe(path):
        return json.loads(subprocess.check_output([
            "ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path),
        ], timeout=25))

    if media == "video":
        def chunk(kind, value):
            data = kind + value
            return struct.pack(">I", len(value)) + data + struct.pack(">I", zlib.crc32(data))
        background = root/"video-layer.png"
        background.write_bytes(
            b"\x89PNG\r\n\x1a\n" +
            chunk(b"IHDR", struct.pack(">IIBBBBB", 16, 16, 8, 2, 0, 0, 0)) +
            chunk(b"IDAT", zlib.compress((b"\x00" + b"\x20\x98\x56"*16)*16)) +
            chunk(b"IEND", b"")
        )
        ui("navigate", view="library")
        context = f"song {song['id']}:"
        page = wait_page(predicate=lambda page: any(context in line and "Create video" in line for line in page.splitlines()))
        row = next(line for line in page.splitlines() if context in line)
        ui("click", ref=re.search(r"(e\d+) Create video", row)[1])
        label = 'button "Render Video (MP4)"'
        wait_page(label)
        tool("video_set", config={"outputSize": 240, "aspectRatio": "1:1"},
             background={"image_path": str(background)})
        click(label)
        target = root/"Клип.mp4"
        save_dialog_file(pid, target, "video-mp4-dialog.json")
        saved(target)
        meta = probe(target)
        video = next(stream for stream in meta["streams"] if stream["codec_type"] == "video")
        audio = next(stream for stream in meta["streams"] if stream["codec_type"] == "audio")
        assert (video["width"], video["height"], video["codec_name"], audio["codec_name"]) == (240, 240, "h264", "aac"), meta
        assert 1.8 < float(meta["format"]["duration"]) < 2.2, meta["format"]
        json_file(out/"video-mp4.json", dict(sha256=digest(target), bytes=target.stat().st_size,
                  background_sha256=digest(background), codecs=["h264", "aac"],
                  dimensions=[video["width"], video["height"]], duration=meta["format"]["duration"]))
        cancel_export(label, "video-mp4")
        failed_save(label, "video-mp4", "mp4")
    else:
        ui("navigate", view="library")
        click('button "DJ mode"', wait_page(title, 'button "DJ mode"'))
        page = wait_page(title, 'button "Record mix"')
        ui("type", ref=control(page, '"Search DJ library"'), value=title)
        only_match = wait_page(predicate=lambda page: page.count('button "A"') == 1 and 'button "B"' in page)
        click('button "A"', only_match)
        click('button "Record mix"')
        play = wait_page(predicate=lambda page: any(
            'button "PLAY"' in line and "disabled" not in line for line in page.splitlines()))
        line = next(line for line in play.splitlines() if 'button "PLAY"' in line and "disabled" not in line)
        ui("click", ref=line.split()[0])
        wait_page('button "PAUSE"')
        time.sleep(1.4)
        finish = 'button "Stop & save recording"'
        retry = 'button "Save recording"'
        cancel_export(finish, "dj-recording", after_label=retry)
        failed_save(retry, "dj-recording", "webm")
        wait_page(retry, "The DJ mix could not be saved")
        click(retry)
        target = root/"Микс.webm"
        save_dialog_file(pid, target, "dj-recording-dialog.json")
        saved(target)
        meta = probe(target)
        audio, = [stream for stream in meta["streams"] if stream["codec_type"] == "audio"]
        assert audio["codec_name"] == "opus", meta
        pcm = subprocess.check_output([
            "ffmpeg", "-v", "error", "-i", str(target), "-ac", "1", "-ar", "16000",
            "-f", "s16le", "pipe:1",
        ], timeout=25)
        samples = [sample for sample, in struct.iter_unpack("<h", pcm)]
        rms = math.sqrt(sum(sample*sample for sample in samples)/len(samples))
        assert len(samples) > 16_000 and rms > 500, (len(samples), rms)
        wait_page('button "Record mix"')
        click('button "Back to Studio"')
        json_file(out/"dj-recording.json", dict(sha256=digest(target), bytes=target.stat().st_size,
                  codec=audio["codec_name"], duration=len(samples)/16_000, rms_pcm16=rms,
                  saved_after_cancel_and_error=True))
    assert stored.read_bytes() == original
    json_file(out/"original-preserved.json", dict(song_id=song["id"], sha256=digest(stored)))


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
        sampler = pathlib.Path(__file__).with_name("sample-installed-visualizer.mjs")
        subprocess.run(["node", str(sampler), mode, str(out)], timeout=30, check=True)
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
    json_file(out/"trainer-status.json", tool("training_status"))
    json_file(out/"rvc-training-status.json", tool("rvc_training_status"))
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


def remove_profile(root, timeout=20):
    deadline = time.monotonic() + timeout
    while True:
        try:
            shutil.rmtree(root)
            return
        except FileNotFoundError:
            # Python 3.12 raises if WebView2 removes a child during traversal.
            if not root.exists():
                return
            if time.monotonic() >= deadline:
                raise
        except OSError as error:
            # WebView2 subprocesses may still hold files after Studio has exited.
            if getattr(error, "winerror", None) not in (5, 32, 145) or time.monotonic() >= deadline:
                raise
        time.sleep(.25)


@contextmanager
def isolated_profile():
    root = pathlib.Path(tempfile.mkdtemp(prefix="studio-hosted-webview-"))
    try:
        yield root
    except BaseException:
        try:
            remove_profile(root)
        except OSError as error:
            print(f"Profile cleanup after test failure: {error}", flush=True)
        raise
    else:
        remove_profile(root)


with isolated_profile() as root:
    seed_training(root)
    for folder in ("user", "roaming", "local"):
        (root/folder).mkdir()
    env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(root), YUE_STUDIO_PORT="8791", DO_NOT_TRACK="1",
        WEBVIEW2_USER_DATA_FOLDER=str(root/"webview"), TIMONCOOL_SHARED_DIR=str(root/"shared"),
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--autoplay-policy=no-user-gesture-required --use-angle=swiftshader --enable-unsafe-swiftshader")
    if os.environ.get("STUDIO_QA_REAL_SHELL_PROFILE") != "1":
        env.update(HOME=str(root/"user"), USERPROFILE=str(root/"user"), APPDATA=str(root/"roaming"),
                   LOCALAPPDATA=str(root/"local"))
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] += " --remote-debugging-port=9889"
    if args.suite == "print" and not os.environ.get("STUDIO_QA_FILE_PORT"):
        # Use Windows' native print output picker on the disposable hosted runner.
        printer_setup = """
$ErrorActionPreference = 'Stop'
Start-Service Spooler
$driver = 'Microsoft Print To PDF'
if (!(Get-PrinterDriver -Name $driver -ErrorAction SilentlyContinue)) { Add-PrinterDriver -Name $driver }
Add-Printer -Name 'Studio QA PDF' -PortName 'PORTPROMPT:' -DriverName $driver
Set-PrintConfiguration -PrinterName 'Studio QA PDF' -PaperSize A4
(New-Object -ComObject WScript.Network).SetDefaultPrinter('Studio QA PDF')
Get-Printer -Name 'Studio QA PDF' | Select-Object Name,DriverName,PortName | ConvertTo-Json
"""
        printer = subprocess.run(["powershell", "-NoProfile", "-Command", printer_setup],
                                 env=dict(env, STUDIO_QA_PDF=str(root/"songbook.pdf")),
                                 capture_output=True, text=True, timeout=60)
        (out/"printer-setup.txt").write_text(printer.stdout + printer.stderr, encoding="utf8")
        printer.check_returncode()
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
            if args.suite in ("lrc", "print", "eqf", "video", "dj"):
                check_lrc(root, process.pid, media=args.suite if args.suite != "lrc" else None)
            else:
                check_screens(root, process.pid)
        finally:
            json_file(out/"trace.json", trace)
            stop = subprocess.Popen([str(args.exe.resolve()), "--stop"], env=env)
            stop.wait(timeout=10)
            try:
                process.wait(timeout=4)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
