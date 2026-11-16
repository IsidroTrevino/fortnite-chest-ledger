using System.Diagnostics;
using System.Globalization;
using System.Reflection.PortableExecutable;
using System.Security.Cryptography;
using System.Text.Json;
using Microsoft.Win32;

namespace TreasureLedger;

static class ReleaseUpdate
{
    const string Repo = "IsidroTrevino/fortnite-chest-ledger";
    const string UpdatingMutex = "Local\\FortniteChestLedgerUpdating";
    const string StandbyMutex = "Local\\FortniteChestLedgerStandby";
    const string UninstallKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\{12DEC791-494D-46DB-BDD4-4E29E6665211}_is1";
    const long MaxBytes = 128L * 1024 * 1024;

    sealed record Package(string Target, string Tag, string File, long Size, string Digest, bool Installer);

    public static async Task<int> Run(string[] args)
    {
        bool check = args[0] == "--check-update";
        if (args.Length != (check ? 4 : 8)) return 2;
        string storage = args[1];
        string target = Path.GetFullPath(args[2]);
        string directory = Path.Combine(storage, "updates");
        Directory.CreateDirectory(directory);

        try
        {
            if (!ParseVersion(args[3], out var current)) return 2;
            if (check)
                await Check(directory, target, current);
            else
                await Apply(directory, storage, target, current, int.Parse(args[4], CultureInfo.InvariantCulture),
                    args[5], args[6] == "restart", args[7] == "standby");
            return 0;
        }
        catch (Exception error)
        {
            File.WriteAllText(Path.Combine(directory, check ? "check-error.txt" : "last-error.txt"), error.ToString());
            return 1;
        }
    }

    static async Task Check(string directory, string target, Version current)
    {
        using var stageLock = LockStage(directory);
        using var client = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false });
        client.DefaultRequestHeaders.UserAgent.ParseAdd("FortniteChestLedger/" + current);
        using var deadline = new CancellationTokenSource(TimeSpan.FromMinutes(3));
        var cancel = deadline.Token;
        using var response = await client.GetAsync($"https://api.github.com/repos/{Repo}/releases/latest", cancel);
        response.EnsureSuccessStatusCode();
        using var release = JsonDocument.Parse(await response.Content.ReadAsStreamAsync(cancel));
        var root = release.RootElement;
        string tag = root.GetProperty("tag_name").GetString() ?? "";
        if (root.GetProperty("draft").GetBoolean() || root.GetProperty("prerelease").GetBoolean() ||
            !tag.StartsWith('v') || !ParseVersion(tag[1..], out var next) || next <= current)
        {
            ClearStage(directory);
            return;
        }

        bool installer = IsInstalled(target);
        string assetName = installer ? "FortniteChestLedger-Setup.exe" : "TreasureLedger.exe";
        var asset = root.GetProperty("assets").EnumerateArray().FirstOrDefault(a =>
            a.GetProperty("name").GetString() == assetName && a.GetProperty("state").GetString() == "uploaded");
        if (asset.ValueKind == JsonValueKind.Undefined)
            throw new InvalidDataException("The latest release does not have an update for this copy of the app.");
        var digest = asset.GetProperty("digest").GetString() ?? "";
        var size = asset.GetProperty("size").GetInt64();
        string url = $"https://github.com/{Repo}/releases/download/{tag}/{assetName}";
        if (size is <= 0 or > MaxBytes || !ValidDigest(digest) || asset.GetProperty("browser_download_url").GetString() != url)
            throw new InvalidDataException("GitHub returned invalid update metadata.");

        string path = Path.Combine(directory, tag + "-" + assetName);
        var package = new Package(target, tag, Path.GetFileName(path), size, digest, installer);
        bool downloaded = !await Verified(path, package, cancel);
        if (downloaded)
        {
            string partial = path + ".partial";
            try
            {
                using var download = await GetAsset(client, new Uri(url), cancel);
                await using (var output = new FileStream(partial, FileMode.Create, FileAccess.Write, FileShare.None))
                await using (var input = await download.Content.ReadAsStreamAsync(cancel))
                {
                    byte[] buffer = new byte[128 * 1024];
                    int count;
                    while ((count = await input.ReadAsync(buffer, cancel)) > 0)
                    {
                        if (output.Length + count > size) throw new InvalidDataException("Update exceeds its declared size.");
                        await output.WriteAsync(buffer.AsMemory(0, count), cancel);
                    }
                }
                if (!await Verified(partial, package, cancel)) throw new InvalidDataException("Update checksum mismatch.");
                File.Move(partial, path, overwrite: true);
            }
            finally { File.Delete(partial); }
        }

        string manifest = Path.Combine(directory, "package.json");
        if (downloaded || !File.Exists(manifest) || Ledger.Read<Package>(manifest) != package)
            File.Delete(Path.Combine(directory, "last-error.txt"));
        Ledger.Write(manifest, package);
        File.WriteAllLines(Path.Combine(directory, "ready.tmp"), [target, tag]);
        File.Move(Path.Combine(directory, "ready.tmp"), Path.Combine(directory, "ready.txt"), overwrite: true);
        foreach (var obsolete in Directory.EnumerateFiles(directory, "v*-*.exe"))
            if (!SamePath(obsolete, path)) File.Delete(obsolete);
    }

    static async Task<HttpResponseMessage> GetAsset(HttpClient client, Uri url, CancellationToken cancel)
    {
        for (int redirects = 0; redirects < 5; redirects++)
        {
            if (url.Scheme != "https" || url.Port != 443 || url.UserInfo.Length != 0 ||
                url.Host is not ("github.com" or "release-assets.githubusercontent.com" or "objects.githubusercontent.com"))
                throw new InvalidDataException("Unexpected GitHub download destination.");
            var response = await client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cancel);
            if (response.IsSuccessStatusCode) return response;
            using (response)
            {
                if ((int)response.StatusCode is >= 300 and < 400 && response.Headers.Location is { } location)
                    url = new Uri(url, location);
                else response.EnsureSuccessStatusCode();
            }
        }
        throw new HttpRequestException("Too many update redirects.");
    }

    static async Task Apply(string directory, string storage, string target, Version current, int parentId,
        string replays, bool restart, bool standby)
    {
        try
        {
            using var maintenance = new Mutex(false, UpdatingMutex, out bool created);
            if (!created) throw new IOException("An update is already being installed.");
            using var parent = FindProcess(parentId);
            if (parent is not null && !parent.WaitForExit(45_000)) throw new IOException("The counter did not close in time.");
            using var stageLock = await WaitForStage(directory);
            var package = Ledger.Read<Package>(Path.Combine(directory, "package.json"));
            if (package is null || !SamePath(target, package.Target) || !package.Tag.StartsWith('v') ||
                !ParseVersion(package.Tag[1..], out var next) || next <= current ||
                package.Installer != IsInstalled(target)) throw new InvalidDataException("Update does not match this copy of the app.");
            string assetName = package.Installer ? "FortniteChestLedger-Setup.exe" : "TreasureLedger.exe";
            if (package.File != package.Tag + "-" + assetName) throw new InvalidDataException("Invalid update package name.");
            string path = Path.Combine(directory, package.File);
            if (!await Verified(path, package, CancellationToken.None)) throw new InvalidDataException("Update checksum mismatch.");

            if (standby)
            {
                for (int attempt = 0; attempt < 50 && Mutex.TryOpenExisting(StandbyMutex, out var watcher); attempt++)
                {
                    watcher.Dispose();
                    await Task.Delay(100);
                }
            }
            if (package.Installer)
            {
                using var installer = Launch(path, ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-",
                    "/DIR=" + Path.GetDirectoryName(target), "/LOG=" + Path.Combine(directory, "setup.log")]);
                await installer.WaitForExitAsync();
                if (installer.ExitCode != 0) throw new IOException("Setup failed with exit code " + installer.ExitCode);
            }
            else
            {
                string replacement = target + "." + Guid.NewGuid().ToString("N") + ".tmp";
                try
                {
                    File.Copy(path, replacement);
                    File.Replace(replacement, target, null);
                }
                finally { File.Delete(replacement); }
            }
            ClearStage(directory);
            File.Delete(Path.Combine(directory, "last-error.txt"));
            File.Delete(path);
        }
        finally
        {
            if (standby) using (Launch(target, ["--standby", "--resume-standby", "--storage", storage, "--replays", replays])) { }
            if (restart) using (Launch(target, ["--storage", storage, "--replays", replays])) { }
        }
    }

    static Process? FindProcess(int id)
    {
        try { return Process.GetProcessById(id); }
        catch (ArgumentException) { return null; }
    }

    static Process Launch(string path, IEnumerable<string> args)
    {
        var start = new ProcessStartInfo(path) { UseShellExecute = false, CreateNoWindow = true };
        foreach (var arg in args) start.ArgumentList.Add(arg);
        return Process.Start(start) ?? throw new IOException("Could not start " + Path.GetFileName(path));
    }

    static FileStream LockStage(string directory) => new(Path.Combine(directory, "update.lock"),
        FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);

    static async Task<FileStream> WaitForStage(string directory)
    {
        for (int attempt = 0; ; attempt++)
        {
            try { return LockStage(directory); }
            catch (IOException) when (attempt < 120) { await Task.Delay(250); }
        }
    }

    static void ClearStage(string directory)
    {
        File.Delete(Path.Combine(directory, "ready.txt"));
        File.Delete(Path.Combine(directory, "package.json"));
    }

    static bool IsInstalled(string target)
    {
        if (!OperatingSystem.IsWindows()) return false;
        if (!Path.GetFileName(target).Equals("TreasureLedger.exe", StringComparison.OrdinalIgnoreCase)) return false;
        using var key = Registry.CurrentUser.OpenSubKey(UninstallKey);
        return key?.GetValue("InstallLocation") is string location &&
            SamePath(Path.TrimEndingDirectorySeparator(location), Path.GetDirectoryName(target)!);
    }

    static bool SamePath(string left, string right) => string.Equals(left, right, StringComparison.OrdinalIgnoreCase);

    static bool ParseVersion(string text, out Version version)
    {
        version = new Version();
        if (!Version.TryParse(text, out var parsed) || parsed.Build < 0 || parsed.Revision >= 0 || text != parsed.ToString(3)) return false;
        version = parsed;
        return true;
    }

    static bool ValidDigest(string digest) => digest.Length == 71 && digest.StartsWith("sha256:", StringComparison.Ordinal) &&
        digest[7..].All(Uri.IsHexDigit);

    static async Task<bool> Verified(string path, Package package, CancellationToken cancel)
    {
        var file = new FileInfo(path);
        if (package.Size is <= 0 or > MaxBytes || !ValidDigest(package.Digest) || !file.Exists || file.Length != package.Size) return false;
        await using var stream = file.OpenRead();
        if (stream.ReadByte() != 'M' || stream.ReadByte() != 'Z') return false;
        if (!package.Installer)
        {
            if (stream.Length < 40) return false;
            stream.Position = 0;
            using var pe = new PEReader(stream, PEStreamOptions.LeaveOpen);
            int certificate = pe.PEHeaders.PEHeader?.CertificateTableDirectory.RelativeVirtualAddress ?? 0;
            long end = certificate > 40 && certificate <= stream.Length ? certificate : stream.Length;
            var footer = new byte[16];
            bool found = false;
            for (int padding = 0; padding <= 7; padding++)
            {
                stream.Position = end - footer.Length - padding;
                await stream.ReadExactlyAsync(footer, cancel);
                if (footer.AsSpan().SequenceEqual("FCLDECODER-V1\0\0\0"u8)) { found = true; break; }
                if (end == stream.Length || footer[^1] != 0) break;
            }
            if (!found) return false;
        }
        stream.Position = 0;
        return Convert.ToHexString(await SHA256.HashDataAsync(stream, cancel))
            .Equals(package.Digest[7..], StringComparison.OrdinalIgnoreCase);
    }
}
