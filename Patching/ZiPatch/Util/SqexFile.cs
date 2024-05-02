/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

namespace FFXIVDownloader.Patching.ZiPatch.Util
{
    public class SqexFile
    {
        public string RelativePath { get; set; }

        protected SqexFile() {}

        public SqexFile(string relativePath)
        {
            RelativePath = relativePath;
        }

        public static string GetExpansionFolder(ushort expansionId) =>
            expansionId == 0 ? "ffxiv" : $"ex{expansionId}";

        public override string ToString() => RelativePath;
    }
}