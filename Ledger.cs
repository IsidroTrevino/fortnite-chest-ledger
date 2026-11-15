using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using ChestCounter;

namespace TreasureLedger;

sealed record ChestCount(int Opened, int Broken)
{
    public int Total => Opened + Broken;
}

sealed record Teammate(string? Player, string Name, ChestCount Count);
sealed record Match(string Replay, string MatchKey, string Player,
    string Name, string Playlist, DateTime StartedUtc, ChestCount Count, bool VehicleGap, Teammate[] Team);
sealed record ReplayEntry(long Bytes, long WriteTicks, string Result, string? Error = null);
sealed record SquadLine(string Name, ChestCount Count, string Source);
sealed record SquadResult(ChestCount Count, SquadLine[] Lines);

sealed class History
{
    public Dictionary<string, ReplayEntry> Files { get; set; } = new(StringComparer.OrdinalIgnoreCase);
    public List<Match> Matches { get; set; } = new();
}

static class Ledger
{
    private static readonly JsonSerializerOptions Json = new() { WriteIndented = true };

    public static T? Read<T>(string path) where T : class
    {
        using var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
        return JsonSerializer.Deserialize<T>(file, Json);
    }

    public static void Write<T>(string path, T value, bool backup = false)
    {
        var payload = JsonSerializer.SerializeToUtf8Bytes(value, Json);
        var temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            File.WriteAllBytes(temporary, payload);
            if (backup && File.Exists(path)) File.Replace(temporary, path, path + ".bak");
            else File.Move(temporary, path, overwrite: true);
        }
        finally
        {
            if (File.Exists(temporary)) File.Delete(temporary);
        }
    }

    public static string Key(string value) => Convert.ToHexString(
        SHA256.HashData(Encoding.UTF8.GetBytes(value.Trim().ToLowerInvariant())));

    public static Match FromReplay(MatchRecord source, FileInfo file)
    {
        var playerId = !string.IsNullOrWhiteSpace(source.EpicId);
        int broken = source.Chests.Count(chest => chest.Destroyed);
        var team = source.TeamChests.Where(mate => !mate.IsBot).Select(mate => new Teammate(
            string.IsNullOrWhiteSpace(mate.EpicId) ? null : Key(mate.EpicId),
            string.IsNullOrWhiteSpace(mate.Name) ? "Squadmate" : mate.Name,
            new ChestCount(mate.Chests - mate.Destroyed, mate.Destroyed))).ToArray();

        return new Match(file.Name, Key(source.SessionId),
            Key(playerId ? source.EpicId! : "name:" + source.PlayerName),
            source.PlayerName ?? "Player", source.Playlist ?? "", source.MatchStartUtc ?? file.LastWriteTimeUtc,
            new ChestCount(source.ChestsOpened - broken, broken), source.VehicleChestsMissing, team);
    }

    public static SquadResult NearbySquad(Match match)
    {
        var lines = new List<SquadLine>
        {
            new(match.Name + " (you)", match.Count, match.VehicleGap ? "Own replay · may miss vehicle breaks" : "Own replay"),
        };
        var seen = new HashSet<string>(StringComparer.Ordinal) { match.Player };
        foreach (var mate in match.Team)
        {
            if (mate.Player is not null && !seen.Add(mate.Player)) continue;
            lines.Add(new(mate.Name, mate.Count, "Nearby · seen in your replay"));
        }
        return new(new ChestCount(lines.Sum(l => l.Count.Opened), lines.Sum(l => l.Count.Broken)), lines.ToArray());
    }
}
