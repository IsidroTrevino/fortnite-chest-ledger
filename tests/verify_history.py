"""Exercise deletion, duplicate replay, local squad counts and backup recovery.

Usage: python verify_history.py TreasureDecoder.exe path-to-finished-squad.replay
All mutations are limited to a new temporary test directory.
"""
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

decoder, source = map(lambda arg: Path(arg).resolve(), sys.argv[1:])
root = Path(tempfile.mkdtemp(prefix="chest-ledger-check-"))
storage, demos, squad = [root / part for part in ("data", "replays", "squad")]
for folder in (storage, demos, squad):
    folder.mkdir()


def scan():
    subprocess.run([str(decoder), str(storage), str(demos)], check=True, timeout=180,
                   creationflags=subprocess.CREATE_NO_WINDOW)
    return json.loads((storage / "history.json").read_text(encoding="utf-8"))


def signature(history):
    return [(m["MatchKey"], m["Player"], m["Count"]["Opened"], m["Count"]["Broken"])
            for m in history["Matches"]]


def first_row():
    return next(line.split("\t") for line in (storage / "snapshot.tsv").read_text(encoding="utf-8").split("\n")
                if line.startswith("match\t"))


shutil.copy2(source, demos / "first.replay")
initial = scan()
assert len(initial["Matches"]) == 1, "Need one readable hunt replay"
match = initial["Matches"][0]
original = signature(initial)
shutil.copy2(source, demos / "renamed.replay")
assert signature(scan()) == original, "Renaming counted a match twice"
for replay in demos.iterdir():
    assert replay.parent.resolve() == demos.resolve()
    replay.unlink()
assert signature(scan()) == original, "Replay deletion removed saved counts"

local_row = first_row()
assert "Nearby · seen in your replay" in local_row[6]
teammate = next(player for player in match["Team"] if player["Player"])
report = {"Version": 1, "Player": teammate["Player"], "Matches": [
    {"MatchKey": match["MatchKey"], "Count": {"Opened": 71, "Broken": 3}, "VehicleGap": False}]}
legacy_report = squad / (teammate["Player"] + ".chestlog.json")
legacy_report.write_text(json.dumps(report), encoding="utf-8")
(storage / "squad-folder.txt").write_text(str(squad), encoding="utf-8")
legacy = scan()
legacy["SquadReports"] = {teammate["Player"]: {match["MatchKey"]: report["Matches"][0]}}
(storage / "history.json").write_text(json.dumps(legacy), encoding="utf-8")
assert signature(scan()) == original, "Old history could not be loaded"
assert first_row() == local_row, "Legacy squad reports changed the nearby total"
assert list(squad.iterdir()) == [legacy_report], "App wrote to the old shared folder"

backup = storage / "history.json.bak"
assert backup.exists(), "Missing automatic backup"
expected_backup = signature(json.loads(backup.read_text(encoding="utf-8")))
(storage / "history.json").write_text('{"broken":', encoding="utf-8")
assert signature(scan()) == expected_backup, "Damaged history did not recover"
assert (storage / "recovery.txt").exists()
assert list(storage.glob("history.json.damaged-*"))
print("PASS: duplicate replay, replay deletion, local squad counts, legacy history, backup recovery")
