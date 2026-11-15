using System.Globalization;
using System.Text;
using ChestCounter;
using TreasureLedger;

if (args.Length != 2) return 2;

string storage = args[0];
string replays = args[1];
Directory.CreateDirectory(storage);
using var instance = new Mutex(false, "Local\\TreasureLedgerDecoder-" + Ledger.Key(storage));
try { if (!instance.WaitOne(TimeSpan.Zero)) return 3; }
catch (AbandonedMutexException) { }

string historyPath = Path.Combine(storage, "history.json");
string progressPath = Path.Combine(storage, "progress.txt");
L.Set("en");
TreasureCatalog.LoadCached(storage);

try
{
    var history = ReplayScan.ReadHistory(historyPath);
    var scanner = new ReplayScan(historyPath, history);
    var scan = new ScanStatus(0, 0, 0);
    string status = Directory.Exists(replays) ? "Watching for finished replays" : "Turn on Settings > Game > Replays > Record Replays";

    SaveSnapshot(scan, status);
    if (Directory.Exists(replays))
    {
        scan = scanner.Run(replays, CancellationToken.None, (index, count) =>
        {
            File.WriteAllText(progressPath, $"Reading replay {index:N0} of {count:N0}");
            if (index == 1 || index % 12 == 0) SaveSnapshot(scan, "Reading saved replays");
        });
        status = scan.Pending > 0 ? "Waiting for Fortnite to finish writing a replay" : status;
    }
    SaveSnapshot(scan, status);
    return 0;

    void SaveSnapshot(ScanStatus state, string message)
    {
        var matches = history.Matches.OrderByDescending(m => m.StartedUtc).ToArray();
        var owner = matches.FirstOrDefault();
        var mine = matches.Where(m => m.Player == owner?.Player).ToArray();
        if (state.Unreadable > 0) message += $"  |  Unreadable: {state.Unreadable}";
        if (state.Skipped > 0) message += $"  |  Outside hunt: {state.Skipped}";
        long opened = mine.Sum(m => (long)m.Count.Opened);
        long broken = mine.Sum(m => (long)m.Count.Broken);
        string average = mine.Length > 0 ? ((opened + broken) / (double)mine.Length).ToString("N1", CultureInfo.InvariantCulture) : "—";
        var output = new StringBuilder();
        Write("summary", owner?.Name ?? "", Number(opened + broken), average, Number(mine.Length), Number(owner?.Count.Total ?? 0), Number(opened), Number(broken));
        Write("state", message);
        long observed = mine.Sum(m => (long)Ledger.NearbySquad(m).Count.Total);
        Write("squad", Number(observed));
        foreach (var match in mine)
        {
            var team = Ledger.NearbySquad(match);
            var local = DateTime.SpecifyKind(match.StartedUtc, DateTimeKind.Utc).ToLocalTime();
            var mode = match.Playlist.ToLowerInvariant() switch
            {
                "playlist_trios" => "Battle Royale Trio",
                _ => match.Playlist.Replace("Playlist_NoBuildBR_", "Zero Build ", StringComparison.OrdinalIgnoreCase)
                    .Replace("Playlist_Default", "Battle Royale ", StringComparison.OrdinalIgnoreCase)
                    .Replace("Playlist_", "Battle Royale ", StringComparison.OrdinalIgnoreCase),
            };
            string details = string.Join("\n", team.Lines.Select(l => $"{l.Name}: {l.Count.Opened} opened, {l.Count.Broken} broken  •  {l.Source}"));
            if (match.VehicleGap) details += "\nYour replay may be missing some chests broken by a vehicle.";
            Write("match", local.ToString("MMM dd  HH:mm", CultureInfo.InvariantCulture), mode,
                Number(match.Count.Opened), Number(match.Count.Broken), Number(team.Count.Total),
                details.Replace('\n', '\u2028'));
        }
        string path = Path.Combine(storage, "snapshot.tsv");
        string content = output.ToString();
        if (File.Exists(path) && File.ReadAllText(path) == content) return;
        string temporary = path + ".new";
        File.WriteAllText(temporary, content);
        File.Move(temporary, path, overwrite: true);

        void Write(params string[] fields) => output.AppendJoin('\t', fields.Select(f => f.Replace('\t', ' ').Replace('\n', ' ').Replace('\r', ' '))).Append('\n');
    }
}
catch (Exception ex)
{
    File.WriteAllText(Path.Combine(storage, "last-error.txt"), ex.ToString());
    return 1;
}
finally
{
    if (File.Exists(progressPath)) File.Delete(progressPath);
    instance.ReleaseMutex();
}

static string Number(long value) => value.ToString("N0", CultureInfo.InvariantCulture);
