import ctypes
import json
import time
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from ctypes import wintypes

def exercise(api, wav, legacy):
    def call(path, body=None, method=None):
        with urlopen(Request(api+path, data=None if body is None else json.dumps(body).encode(), method=method,
                             headers={"Content-Type":"application/json"}),timeout=30) as response:
            return json.load(response)
    song=call("/v1/library/songs",dict(title="Locked artwork",audio_path=str(wav),engine_id="manual",metadata={},caption="",lyrics=""))
    route="/v1/library/songs/"+song["id"]
    with_cover=call(route+"/cover",{"image_base64":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jQn8AAAAASUVORK5CYII=","media_type":"image/png"},"PUT")
    cover=wav.parent/with_cover["metadata"]["cover_filename"]
    assert cover.is_file()
    call(route+"/process",{"normalize":{"peak_clip":1}})
    deadline=time.monotonic()+90
    while not call("/v1/processing")["run"]["preview_ready"]:
        assert time.monotonic()<deadline
        time.sleep(.1)
    kernel=ctypes.WinDLL("kernel32",use_last_error=True)
    kernel.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE]
    kernel.CreateFileW.restype=wintypes.HANDLE
    kernel.CloseHandle.argtypes=[wintypes.HANDLE]
    handle=kernel.CreateFileW(str(cover),0x80000000,0,None,3,0,None)
    assert handle!=ctypes.c_void_p(-1).value,ctypes.get_last_error()
    try:
        try:
            call("/v1/processing/keep",{"label":"Locked artwork test"})
            status=200
        except HTTPError as exc:
            status=exc.code
            assert b"cover" in exc.read()
    finally:
        kernel.CloseHandle(handle)
    songs=[s for s in call("/v1/library/songs") if s["title"].endswith("Locked artwork test")]
    assert len(songs)==1,songs
    exists=Path(songs[0]["audio_path"]).is_file()
    assert status==(500 if legacy else 200) and exists!=legacy,(status,exists)
    assert cover.is_file() and wav.is_file()
    return dict(status=status, audio_exists=exists,derived_id=songs[0]["id"],source_id=song["id"])
