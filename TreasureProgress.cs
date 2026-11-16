using System.Globalization;
using ChestCounter;

namespace TreasureLedger;

sealed record RecordedReward(string Key, float? Seconds, bool Shared);
sealed record RewardCard(string Id, string Name, string Category, string Group, int Count, string Latest);

static class TreasureProgress
{
    const string Unknown = "unidentified";
    static readonly (string Id, string Name, string Item)[] Gizmos =
    [
        ("portable-extractor", "Portable Extractor", "Extracteur portable"),
        ("extraction-accelerator", "Extraction Accelerator", "Accélérateur d'extraction"),
        ("sprite-locator", "Sprite Locator", "Localisateur d'esprit"),
        ("lucky-locator", "Lucky Locator", "Localisateur chanceux"),
        ("golden-firework", "Golden Firework", "Feu d'artifice doré"),
    ];

    public static RecordedReward[] FromReplay(MatchRecord match)
    {
        return Merge(Finds(match.TreasureFinds, false).ToArray(), Finds(match.TeamTreasureFinds, true).ToArray());

        static IEnumerable<RecordedReward> Finds(IEnumerable<TreasureFind> finds, bool shared)
        {
            foreach (var find in finds)
            {
                if (find.FromGoldenChest && !string.IsNullOrWhiteSpace(find.PrizeId))
                {
                    yield return new(find.PrizeId, find.ObtainedAtSeconds, shared);
                }
                else if (find.FromPrizeHandler && find.SourceIsChest)
                {
                    var gizmo = Gizmos.FirstOrDefault(g => g.Item == find.Item);
                    yield return new(gizmo.Id ?? Unknown, find.ObtainedAtSeconds, shared);
                }
            }
        }
    }

    public static RecordedReward[] Merge(RecordedReward[]? previous, RecordedReward[]? current)
    {
        return (previous ?? []).Concat(current ?? []).DistinctBy(r => (r.Key.ToLowerInvariant(), r.Seconds))
            .OrderBy(r => r.Seconds).ToArray();
    }

    public static RewardCard[] Cards(IReadOnlyList<Match> matches)
    {
        var events = matches.SelectMany(match => (match.Rewards ?? []).Select(reward => (Match: match, Drop: reward)))
            .ToLookup(x => TreasureCatalog.Find(x.Drop.Key)?.Id ?? x.Drop.Key, StringComparer.OrdinalIgnoreCase);
        var cards = new List<RewardCard>();
        var token = TreasureCatalog.PirateShopAccess;
        cards.Add(Card(token.Id, token.Name, "Token", "All season"));
        foreach (var week in TreasureCatalog.Weeks)
            foreach (var prize in week.Prizes)
                cards.Add(Card(prize.Id, prize.Name, Category(prize), $"Week {week.Number}"));
        foreach (var gizmo in Gizmos) cards.Add(Card(gizmo.Id, gizmo.Name, "Gizmo", "All season"));
        var listed = cards.Select(c => c.Id).ToHashSet(StringComparer.OrdinalIgnoreCase);
        foreach (var group in events.Where(g => !listed.Contains(g.Key) && g.Key != Unknown))
            cards.Add(Card(group.Key, "Uncatalogued treasure", "Recorded prize ID", "Mystery drop"));
        if (events.Contains(Unknown)) cards.Add(Card(Unknown, "Unidentified treasure", "Reward type unavailable", "Replay detections"));
        return cards.ToArray();

        RewardCard Card(string id, string name, string category, string group)
        {
            var found = events[id].ToArray();
            if (found.Length == 0) return new(id, name, category, group, 0, "");
            var latest = found[0];
            var date = DateTime.SpecifyKind(latest.Match.StartedUtc, DateTimeKind.Utc).ToLocalTime();
            string detail = date.ToString("MMM dd, yyyy", CultureInfo.InvariantCulture) + (latest.Drop.Shared ? " · squad" : "");
            return new(id, name, category, group, found.Length, detail);
        }
    }

    static string Category(TreasurePrize prize) => prize.Kind == TreasureCatalog.PickaxeOrBackBling
        ? (prize.Id.EndsWith("backbling", StringComparison.Ordinal) ? "Back bling" : "Pickaxe / back bling")
        : TreasureCatalog.KindName(prize.Kind);

    public static (long Solo, long Team, string Chance, int OneIn) Odds(IReadOnlyList<Match> matches)
    {
        long solo = 0, team = 0;
        foreach (var match in matches)
        {
            if (match.Playlist.Contains("solo", StringComparison.OrdinalIgnoreCase)) solo += match.Count.Opened;
            else team += Ledger.NearbySquad(match).Count.Opened;
        }
        int oneIn = TreasureCatalog.Odds.Single(x => x.Kind == TreasureCatalog.PirateShop).OneIn;
        double percent = TreasureCatalog.AtLeastOnce(oneIn, solo + team) * 100;
        string chance = percent > 99.999 ? ">99.999%" : percent.ToString("0.000", CultureInfo.InvariantCulture) + "%";
        return (solo, team, chance, oneIn);
    }
}
