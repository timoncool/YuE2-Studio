"""Exercise a selective, partially successful migration through the actual MCP service."""
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
from urllib.request import ProxyHandler, Request, build_opener
import wave


def seed_source(root):
    media = root / "media"
    media.mkdir(parents=True)
    with sqlite3.connect(root / "library.sqlite") as connection:
        connection.executescript("""
            CREATE TABLE songs(id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,
                engine_id,profile_id,replay_request_json,audio_codes_json,source,created_at,updated_at);
            CREATE TABLE playlists(id,name,description,created_at,updated_at);
            CREATE TABLE playlist_songs(playlist_id,song_id,position);
            INSERT INTO songs VALUES('healthy','Healthy','healthy.wav','folk','words to keep',
                '{"cover_filename":"not-on-disk.png","lrc":"[00:00.50]word"}','{"seed":42}',
                'acestep-cpp',NULL,NULL,NULL,'local_generation','2','2');
            INSERT INTO songs VALUES('missing','Missing song','missing.wav','','','{}','{}',
                'acestep-cpp',NULL,NULL,NULL,'local_generation','1','1');
            INSERT INTO playlists VALUES('show','Set list',NULL,'1','1');
            INSERT INTO playlist_songs VALUES('show','missing',0),('show','healthy',1);
        """)
    connection.close()
    write_wave(media / "healthy.wav")
    write_wave(media / "healthy-drums.wav")


def write_wave(path):
    with wave.open(str(path), "wb") as audio:
        audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        audio.writeframes(b"\x10\x00" * 16000)


def snapshot(root):
    return {str(file.relative_to(root)): hashlib.sha256(file.read_bytes()).hexdigest()
            for file in root.rglob("*") if file.is_file()}


def exercise(executable, root):
    source, data = root / "old", root / "new"
    seed_source(source)
    original = snapshot(source)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    opener = build_opener(ProxyHandler({}))

    def request(path, payload=None, method=None):
        body = json.dumps(payload).encode("utf8") if payload is not None else None
        headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
        with opener.open(Request(f"http://127.0.0.1:{port}{path}", data=body, headers=headers, method=method), timeout=20) as reply:
            return json.load(reply)

    def tool(name, **arguments):
        response = request("/mcp", {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                                    "params": {"name": name, "arguments": arguments}})
        assert not response["result"].get("isError"), response
        return response["result"]["structuredContent"]

    env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(data), YUE_STUDIO_PORT=str(port),
               TIMONCOOL_SHARED_DIR=str(root / "shared"), DO_NOT_TRACK="1")
    for name in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA"):
        location = root / name.lower()
        location.mkdir()
        env[name] = str(location)
    env.pop("YUE_ENGINE_BASE_URL", None)
    with (root / "service.log").open("wb") as log:
        process = subprocess.Popen([str(executable)], cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            for _ in range(120):
                assert process.poll() is None, process.returncode
                try:
                    request("/health")
                    break
                except OSError:
                    time.sleep(.25)
            else:
                raise TimeoutError("Studio did not start")
            args = {"path": str(source), "exclude": ["covers", "stems", "karaoke"]}
            preview = tool("library_migration_preview", path=str(source))
            assert any(file["name"] == "missing.wav" for song in preview["songs"] for file in song["missing"]), preview
            initial = tool("library_migration_import", **args)
            assert initial["added"] == 1 and initial["already_imported"] == 0, initial
            assert initial["playlists_added"] == 0, initial
            assert [(entry["file"], entry["code"]) for entry in initial["errors"]] == [
                ("missing.wav", "missing"), ("Set list", "playlist_incomplete")
            ], initial
            songs = request("/v1/library/songs")
            healthy, = songs
            assert healthy["title"] == "Healthy" and healthy["lyrics"] == "words to keep", healthy
            with opener.open(f"http://127.0.0.1:{port}/v1/library/media/{healthy['id']}", timeout=10) as audio:
                assert audio.read() == (source / "media" / "healthy.wav").read_bytes()
            assert "lrc" not in healthy["metadata"] and "cover_filename" not in healthy["metadata"], healthy
            assert not list((data / "media").glob("*-drums.wav"))
            assert snapshot(source) == original
            write_wave(source / "media" / "missing.wav")
            before_retry = snapshot(source)
            retry = tool("library_migration_import", **args)
            assert retry["added"] == retry["already_imported"] == retry["playlists_added"] == 1, retry
            assert retry["errors"] == [], retry
            final = request("/v1/library/songs")
            assert next(song for song in final if song["title"] == "Healthy")["id"] == healthy["id"], final
            missing_id = next(song["id"] for song in final if song["title"] == "Missing song")
            playlists = request("/v1/library/playlists")
            playlist, = playlists if isinstance(playlists, list) else playlists["playlists"]
            assert playlist["name"] == "Set list" and playlist["song_ids"] == [missing_id, healthy["id"]], playlist
            duplicate = tool("library_migration_import", **args)
            assert duplicate["added"] == duplicate["playlists_added"] == 0, duplicate
            assert snapshot(source) == before_retry
            metadata_source = root / "metadata-only"
            seed_source(metadata_source)
            first = tool("library_migration_import", path=str(metadata_source),
                         exclude=["audio", "covers", "stems", "karaoke", "playlists"])
            assert first["added"] == 2 and first["errors"] == [], first
            text_only = next(song for song in request("/v1/library/songs")
                             if song["title"] == "Healthy" and song["id"] != healthy["id"])
            assert text_only["audio_path"] is None, text_only
            request(f"/v1/library/songs/{text_only['id']}/title", {"title": "Edited locally"}, method="PUT")
            more = tool("library_migration_import", path=str(metadata_source),
                        exclude=["covers", "stems", "karaoke", "playlists"])
            assert more["added"] == 0 and more["updated"] == 1 and more["already_imported"] == 2, more
            assert more["errors"][0]["file"] == "missing.wav", more
            filled = request(f"/v1/library/songs/{text_only['id']}")
            assert filled["title"] == "Edited locally" and filled["audio_path"], filled
            with opener.open(f"http://127.0.0.1:{port}/v1/library/media/{text_only['id']}", timeout=10) as audio:
                assert audio.read() == (metadata_source / "media" / "healthy.wav").read_bytes()
            print(json.dumps({"initial": initial, "after_repair": retry, "repeat": duplicate,
                              "audio_backfill": more, "same_song_id_with_local_edit": True,
                              "playlist_preserved_order": True, "source_hashes_unchanged": True}))
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="legacy-library-mixed-") as directory:
        exercise(Path(sys.argv[1]).resolve(strict=True), Path(directory))

