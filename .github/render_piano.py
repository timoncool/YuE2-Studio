from pathlib import Path
import subprocess
root=Path.cwd()
notes=[60,64,67,72]
# General MIDI acoustic grand, four quarter notes with gaps at 60 BPM.
import struct
track=bytearray.fromhex("00 ff 51 03 0f 42 40 00 c0 00")
for pitch in notes:
    track.extend((24,0x90,pitch,85,72,0x80,pitch,0))
track.extend((96,255,47,0))
mid=root/"piano.mid"
mid.write_bytes(b"MThd"+struct.pack(">IHHH",6,0,1,96)+b"MTrk"+struct.pack(">I",len(track))+track)
audio=root/"piano.wav"
subprocess.run(["fluidsynth","-ni","-g","0.7","-r","24000","-F",str(audio),"/usr/share/sounds/sf2/FluidR3_GM.sf2",str(mid)],check=True)
