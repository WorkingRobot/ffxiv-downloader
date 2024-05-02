/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Patching.ZiPatch.Util;

namespace FFXIVDownloader.Patching.ZiPatch
{
    public abstract class ZiPatchConfig
    {
        public enum PlatformId : ushort
        {
            Win32 = 0,
            Ps3 = 1,
            Ps4 = 2,
            Unknown = 3
        }

        public PlatformId Platform { get; set; }
        public bool IgnoreMissing { get; set; }
        public bool IgnoreOldMismatch { get; set; }

        public abstract Stream OpenStream(SqexFile file);

        public abstract void CreateDirectory(string path);

        public abstract void DeleteFile(string path);

        public abstract void DeleteDirectory(string path);

        public abstract void DeleteExpansion(ushort expansionId);
    }

    public sealed class PersistentZiPatchConfig : ZiPatchConfig, IDisposable
    {
        private Dictionary<string, FileStream> Streams { get; }
        private string GamePath { get; }

        public PersistentZiPatchConfig(string gamePath)
        {
            GamePath = Path.GetFullPath(gamePath);
            Streams = [];
        }

        public override Stream OpenStream(SqexFile file)
        {
            var path = Path.Combine(GamePath, file.RelativePath);

            if (Streams.TryGetValue(path, out var stream))
                return stream;

            if (Path.GetDirectoryName(path) is { } dirName)
                Directory.CreateDirectory(dirName);

            int tries = 5;
            int sleeptime = 1;
            do
            {
                try
                {
                    stream = new FileStream($"{GamePath}/{file.RelativePath}", FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.Read, 1 << 16);
                    Streams.Add(path, stream);
                }
                catch (IOException)
                {
                    if (tries == 0)
                        throw;

                    Thread.Sleep(sleeptime * 1000);
                }
            } while (0 < --tries);

            throw new FileNotFoundException($"Could not find file {file.RelativePath}");
        }

        public override void CreateDirectory(string path)
        {
            Directory.CreateDirectory($"{GamePath}/{path}");
        }

        public override void DeleteFile(string path)
        {
            path = $"{GamePath}/{path}";
            if (Streams.Remove(path, out var stream))
                stream.Dispose();
            File.Delete(path);
        }

        public override void DeleteDirectory(string path)
        {
            Directory.Delete(path);
        }

        public override void DeleteExpansion(ushort expansionId)
        {
            var expansionFolder = SqexFile.GetExpansionFolder(expansionId);

            var sqpack = $@"{GamePath}\sqpack\{expansionFolder}";
            var movie = $@"{GamePath}\movie\{expansionFolder}";

            var shouldKeep = (string f) => new[] { ".var", "00000.bk2", "00001.bk2", "00002.bk2", "00003.bk2" }.Any(f.EndsWith);

            if (Directory.Exists(sqpack))
            {
                foreach (var file in Directory.GetFiles(sqpack))
                {
                    if (!shouldKeep(file))
                    {
                        if (Streams.Remove(file, out var stream))
                            stream.Dispose();
                        File.Delete(file);
                    }
                }
            }

            if (Directory.Exists(movie))
            {
                foreach (var file in Directory.GetFiles(movie))
                {
                    if (!shouldKeep(file))
                    {
                        if (Streams.Remove(file, out var stream))
                            stream.Dispose();
                        File.Delete(file);
                    }
                }
            }
        }

        public void Dispose()
        {
            foreach (var stream in Streams.Values)
                stream.Dispose();
        }
    }
}