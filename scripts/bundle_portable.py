"""Append the self-contained decoder to a compiled TreasureLedger.exe."""
import hashlib
import struct
import sys
from pathlib import Path

MAGIC = b"FCLDECODER-V1\0\0\0"


def main():
    launcher, decoder, output = map(Path, sys.argv[1:])
    native = launcher.read_bytes()
    payload = decoder.read_bytes()
    if native[-len(MAGIC):] == MAGIC or not native.startswith(b"MZ") or not payload.startswith(b"MZ"):
        raise ValueError("Expected an unbundled Windows launcher and a Windows decoder")
    identity = hashlib.sha256(payload).hexdigest()[:16].encode("ascii")
    footer = struct.pack("<Q16s16s", len(payload), identity, MAGIC)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(native + payload + footer)


if __name__ == "__main__":
    main()
