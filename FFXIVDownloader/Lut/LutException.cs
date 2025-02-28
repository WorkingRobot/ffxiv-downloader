namespace FFXIVDownloader.Lut;

public class LutException(string message, Exception? innerException = null) :
    Exception($"Lut: {message}", innerException)
{
}
