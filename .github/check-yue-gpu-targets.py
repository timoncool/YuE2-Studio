import json, os, socket, subprocess, sys, tempfile, time
from pathlib import Path
from urllib.request import urlopen

exe=Path(sys.argv[1]).resolve(strict=True)
old="--baseline" in sys.argv
temp=tempfile.TemporaryDirectory(prefix="yue-cuda-targets-")
root=Path(temp.name)
import shutil
shutil.copyfile(exe,root/"music-server.exe")
src=root/"smi.rs"
src.write_text("""
fn main() {
    let query = std::env::args().collect::<Vec<_>>().join(" ");
    if query.contains("compute_cap,driver_version") {
        println!("{}, 590.10",std::env::var("QA_COMPUTE").unwrap());
    } else if query.contains("name,memory.total") {
        println!("NVIDIA QA, 24576");
    }
}
""")
subprocess.run(["rustc",src,"-o",root/"nvidia-smi.exe"],check=True)
def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1",0))
        return s.getsockname()[1]
for compute,card,engine in [("8.9","cuda13","cuda13"),("12.0","cuda13","cuda13"),
                            ("10.0","cuda13",None),("12.1","cuda13",None),("6.1","cuda12","cuda12")]:
    home=root/compute
    home.mkdir()
    api_port=port()
    env=dict(os.environ,QA_COMPUTE=compute,PATH=str(root)+os.pathsep+os.environ["PATH"],
             YUE_STUDIO_DATA_ROOT=str(home),YUE_MODELS_ROOT=str(home/"models"),YUE_STUDIO_SETTINGS_PATH=str(home/"studio-settings.json"),
             YUE_STUDIO_PORT=str(api_port),YUE_ENGINE_BIN=str(home/"missing.exe"),YUE_ENGINE_PORT=str(port()),STUDIO_TELEMETRY="0")
    with (home/"service.log").open("wb") as log:
        proc=subprocess.Popen([root/"music-server.exe"],cwd=root,env=env,stdout=log,stderr=log)
        try:
            for _ in range(120):
                if proc.poll() is not None: raise RuntimeError((home,proc.returncode))
                try:
                    with urlopen(f"http://127.0.0.1:{api_port}/setup/status",timeout=8) as r: hw=json.load(r)["hardware"]
                    break
                except OSError:time.sleep(.2)
            else:raise TimeoutError(home)
            assert hw["gpuName"]=="NVIDIA QA" and hw["computeCapability"]==list(map(int,compute.split("."))),hw
            assert hw["runsCuda"] and hw["cardCuda"]==card,hw
            expected=card if old else engine
            assert hw["cuda"]==expected and hw["vulkan"]==(expected is None),hw
            def get(route):
                with urlopen(f"http://127.0.0.1:{api_port}"+route,timeout=15) as r:return json.load(r)
            midi=get("/v1/midi")
            runtime=get("/v1/midi/runtime")
            stems=get("/v1/separation/status")
            wanted=engine or "cpu"
            assert midi["runtime_build"]==wanted and midi["error"] is None,midi
            servers=[a["label"] for a in runtime["assets"] if a["label"].startswith("audio.cpp server")]
            assert servers==["audio.cpp server ("+wanted.capitalize()+")"],servers
            assert stems["card"]==("cuda" if engine else "vulkan"),stems["card"]
            print(json.dumps(dict(compute=compute,card=hw["cardCuda"],engine=hw["cuda"],vulkan=hw["vulkan"],
                                  midi_build=midi["runtime_build"],midi_server=servers[0],stem_gpu=stems["card"])),flush=True)
        finally:
            proc.terminate()
            proc.wait(timeout=10)
