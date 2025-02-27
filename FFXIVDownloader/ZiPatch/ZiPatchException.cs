/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

namespace FFXIVDownloader.ZiPatch;

public class ZiPatchException(string message, Exception? innerException = null) :
    Exception($"ZiPatch: {message}", innerException)
{
}
