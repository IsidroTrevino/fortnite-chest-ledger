# Fortnite Chest Ledger

A Windows x64 chest counter for Fortnite Treasure Hunters. It reads completed Fortnite replays on your PC and keeps a local match history.

## Download

Get the files from [the latest release](https://github.com/IsidroTrevino/fortnite-chest-ledger/releases/latest).

| Download | How to use it |
| --- | --- |
| `FortniteChestLedger-Setup.exe` | Run the installer. It adds a Start menu entry and offers a desktop shortcut. |
| `TreasureLedger.exe` | Move the single executable to your desktop or a folder of your choice and double-click it. |

Both run on Windows 10 or 11 x64 and include the replay decoder. If you're updating from an older ZIP, close the old counter before opening this version. The app uses your existing local history.

## Automatic updates

Automatic updates are available in version 1.3.0 and later. The counter checks GitHub on launch and every six hours while open. A newer stable release downloads in the background and installs when you close the counter. Use **Restart to update** to apply a ready update and reopen the counter immediately. Installed copies use the setup package and portable copies replace the executable in place. An offline PC can keep counting and will retry after it reconnects.

Select **Check for updates** in the sidebar for an immediate check while the dashboard stays open. The status below the button shows when the check finishes or an update is ready.

## Start tracking

1. In Fortnite, turn on **Settings → Game → Replays → Record Replays**.
2. Open **Fortnite Chest Ledger** and leave it open or minimized. The first scan imports supported replays already on your PC. New counts appear after Fortnite saves each match.
3. For the click-through overlay, set **Settings → Video → Window Mode → Windowed Fullscreen**. Toggle the overlay in the sidebar.
4. Enable **Open with Fortnite** if you'd like the counter to start minimized when Fortnite starts. A small watcher runs at Windows sign-in. Put the portable executable in its permanent location before enabling this option.

The dashboard counts opened and broken chests in supported unranked Battle Royale and Zero Build Treasure Hunters matches. **Combined chests opened** adds your solo openings to your openings and nearby teammate openings in team matches. Teammates on the other side of the map can be outside your replay's view. The match list's **Squad seen** column includes breaks and opens from you and your observed teammates. Counts can miss replay omissions such as some vehicle breaks.

**Pirate Shop token chance** estimates the chance of at least one drop across the combined opens using the published normal rate of 1 in 137,500. It computes `1 - (1 - 1/137500)^n` for `n` recorded openings. Breaks are excluded. Treasure Hours can increase the actual odds.

## Treasure rewards

Open **Treasure rewards** to see the Pirate Shop token, weekly skins and other cosmetics, and Treasure Hunt Gizmos. The app marks identifiable drops as found when your completed replays record them, including shared squad prizes present in your replay. Repeated finds show a count. Prize events that the decoder cannot name remain visible as unidentified detections.

Version 1.4.0 scans previously counted replays that are still on disk for rewards. Saved counts and identified finds survive replay deletion. If a replay disappeared before a reward scan, its match keeps its counts and is listed in the number of saved matches without reward data. Future weekly prizes appear as placeholders until their artwork and names are available in a release.

## Match history

Once processed, a match stays in your history after the original replay is deleted. Renamed copies of a known replay are deduplicated when the replay contains the same match identifier. Use **Open history folder** to access `history.json` and its previous successful version, `history.json.bak`. The history is stored under `%LOCALAPPDATA%\FortniteChestLedger` and is kept when you uninstall. If the main history is damaged, the app recovers from the backup when available. A replay deleted before its first successful scan cannot be recovered.

## Build from source

You need the Windows SDK, Rust for the MSVC target, and the .NET 10 SDK. Run the build in a Visual Studio Developer PowerShell so Cargo can find `rc.exe`. Python 3 assembles the standalone executable. Inno Setup 7 builds the optional installer.

Build from the repository root in PowerShell.

```powershell
cargo build --release --manifest-path native/Cargo.toml
dotnet publish TreasureLedger.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:EnableCompressionInSingleFile=true -o dist/decoder
python scripts/bundle_portable.py native/target/release/treasure-ledger.exe dist/decoder/TreasureDecoder.exe dist/TreasureLedger.exe
Copy-Item LICENSE-ChestCounter.txt dist/
Copy-Item lib/LICENSE-FortniteReplayDecompressor dist/LICENSE-FortniteReplayDecompressor.txt
Copy-Item "$env:ProgramFiles\dotnet\LICENSE.txt" dist/LICENSE-dotnet.txt
Copy-Item "$env:ProgramFiles\dotnet\ThirdPartyNotices.txt" dist/ThirdPartyNotices-dotnet.txt
ISCC.exe installer/TreasureLedger.iss
```

To publish an update, bump the version in `native/Cargo.toml` and `installer/TreasureLedger.iss`, rebuild both executables, and attach `TreasureLedger.exe` and `FortniteChestLedger-Setup.exe` to a stable GitHub release tagged `vX.Y.Z`. Keep the asset filenames unchanged. The app uses the latest stable release and verifies each asset against the SHA-256 digest published by GitHub.

Current downloads are unsigned. For a signed release, Authenticode-sign `TreasureDecoder.exe` before bundling it, then sign the completed portable executable before compiling the installer. Configure Inno Setup's signing tool and pass `/DSignProfile=your-tool-name` to sign Setup and its uninstaller. Use the same trusted publisher identity for subsequent releases and sign every binary before uploading. See [Microsoft's SmartScreen guidance](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation).

The app reads the default Fortnite replay folder. Run `TreasureLedger.exe --replays "C:\path\to\Demos"` to use a custom location. For an integration check, run `python tests/verify_history.py dist/decoder/TreasureDecoder.exe "C:\path\to\a-finished-squad.replay"`.

## Credits and license

The project is independent of Epic Games. Replay analysis adapts [LED-N/Fortnite-ChestCounter v1.5.1](https://github.com/LED-N/Fortnite-ChestCounter/tree/v1.5.1) and [Shiqan/FortniteReplayDecompressor](https://github.com/Shiqan/FortniteReplayDecompressor) under their MIT licenses. See [the Chest Counter license](LICENSE-ChestCounter.txt) and [the replay decoder license](lib/LICENSE-FortniteReplayDecompressor).

The app icon is an original front-view illustration based on the small gold Treasure Hunters chest found on the map. Fortnite belongs to Epic Games.

Reward art is sourced from [Fortnite Bot](https://fortnite.bot/treasure) and [9NTY](https://9ntyapi.com/treasure). Individual asset links are in [the image source list](assets/rewards/sources.json).
