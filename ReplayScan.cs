using System.Text.Json;
using ChestCounter;

namespace TreasureLedger;

sealed record ScanStatus(int Pending, int Unreadable, int Skipped);

sealed class ReplayScan(string storage, History history)
{
    public ScanStatus Run(string folder, CancellationToken cancellation, Action<int, int>? progress = null)
    {
        var files = new DirectoryInfo(folder).GetFiles("*.replay").OrderByDescending(f => f.LastWriteTimeUtc).ToArray();
        int pending = 0;
        int unreadable = 0;
        int skipped = 0;
        for (int index = 0; index < files.Length; index++)
        {
            cancellation.ThrowIfCancellationRequested();
            var file = files[index];
            var name = file.FullName;
            file.Refresh();
            if (!file.Exists) continue;

            var entry = new ReplayEntry(file.Length, file.LastWriteTimeUtc.Ticks, "counted");
            if (history.Files.TryGetValue(name, out var previous) && previous.Bytes == entry.Bytes && previous.WriteTicks == entry.WriteTicks)
            {
                if (previous.Result == "unreadable") unreadable++;
                if (previous.Result == "skipped") skipped++;
                continue;
            }

            var age = DateTime.UtcNow - file.LastWriteTimeUtc;
            if (age < TimeSpan.FromSeconds(6) || (ReplayHeaderProbe.IsLive(name) == true && age < TimeSpan.FromMinutes(30)))
            {
                pending++;
                continue;
            }

            try
            {
                if (ReplayHeaderProbe.KeyMissing(name)) throw new InvalidDataException("Fortnite did not save the replay decryption key.");
                using var stream = new FileStream(name, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                var result = new ReplayAnalyzer().Analyze(stream, name, username: null);
                if (GameModes.Counts(result))
                {
                    if (!result.IsReplayOwner || (string.IsNullOrWhiteSpace(result.EpicId) && string.IsNullOrWhiteSpace(result.PlayerName)))
                        throw new InvalidDataException("The replay owner could not be identified.");
                    var match = Ledger.FromReplay(result, file);
                    int found = history.Matches.FindIndex(m => m.MatchKey == match.MatchKey && m.Player == match.Player);
                    if (found < 0) history.Matches.Add(match);
                    else if (match.StartedUtc == history.Matches[found].StartedUtc &&
                             match.Count.Total > history.Matches[found].Count.Total) history.Matches[found] = match;
                }
                else if (string.IsNullOrWhiteSpace(result.Playlist) && (result.Map is null || GameModes.IsSeasonMap(result.Map)))
                {
                    throw new InvalidDataException("The replay does not contain enough information to identify the mode.");
                }
                else
                {
                    skipped++;
                    entry = entry with { Result = "skipped", Error = GameModes.WhyNot(result) };
                }
            }
            catch (IOException) when (age < TimeSpan.FromSeconds(45))
            {
                pending++;
                continue;
            }
            catch (Exception ex) when (ex is not OutOfMemoryException and not OperationCanceledException)
            {
                unreadable++;
                entry = entry with { Result = "unreadable", Error = ex.GetType().Name + ": " + ex.Message };
            }
            history.Files[name] = entry;
            Ledger.Write(storage, history, backup: true);
            progress?.Invoke(index + 1, files.Length);
        }
        return new(pending, unreadable, skipped);
    }

    public static History ReadHistory(string path)
    {
        if (!File.Exists(path) && !File.Exists(path + ".bak")) return new();
        try { return ReadValid(path); }
        catch (Exception ex) when (ex is JsonException or IOException)
        {
            var recovered = ReadValid(path + ".bak");
            if (File.Exists(path)) File.Copy(path, path + ".damaged-" + DateTime.UtcNow.ToString("yyyyMMddHHmmssfff"));
            Ledger.Write(path, recovered);
            File.WriteAllText(Path.Combine(Path.GetDirectoryName(path)!, "recovery.txt"),
                "Recovered the previous history backup at " + DateTime.UtcNow.ToString("O"));
            return recovered;
        }

        static History ReadValid(string source)
        {
            using var document = JsonDocument.Parse(File.ReadAllBytes(source));
            if (!document.RootElement.TryGetProperty("Matches", out _) || !document.RootElement.TryGetProperty("Files", out _))
                throw new JsonException("Invalid match history.");
            var result = Ledger.Read<History>(source) ?? throw new JsonException("Empty history.");
            if (result.Matches is null || result.Files is null) throw new JsonException("Invalid history fields.");
            return result;
        }
    }
}
