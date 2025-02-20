using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace FFXIVDownloader.Thaliak;

public readonly record struct ParsedVersionString : IComparable<ParsedVersionString>, IEquatable<ParsedVersionString>, IFormattable
{
    public int Year { get; init; }
    public int Month { get; init; }
    public int Day { get; init; }
    public int Part { get; init; }
    public int Revision { get; init; }
    public bool IsHistoric { get; init; }
    public string? Section { get; init; }

    public ParsedVersionString(string versionString)
    {
        if (versionString.StartsWith('H'))
        {
            IsHistoric = true;
            versionString = versionString[1..];
        }
        else if (versionString.StartsWith('D'))
        {
            IsHistoric = false;
            versionString = versionString[1..];
        }

        while (char.IsAsciiLetterLower(versionString[^1]))
        {
            Section ??= string.Empty;
            Section = versionString[^1] + Section;
            versionString = versionString[..^1];
        }

        var parts = versionString.Split('.');
        if (parts.Length != 5)
            throw new ArgumentException("Invalid version string", nameof(versionString));

        Year = int.Parse(parts[0]);
        Month = int.Parse(parts[1]);
        Day = int.Parse(parts[2]);
        Part = int.Parse(parts[3]);
        Revision = int.Parse(parts[4]);
    }

    public readonly int CompareTo(ParsedVersionString other)
    {
        if (Year != other.Year)
            return Year.CompareTo(other.Year);

        if (Month != other.Month)
            return Month.CompareTo(other.Month);

        if (Day != other.Day)
            return Day.CompareTo(other.Day);

        if (Part != other.Part)
            return Part.CompareTo(other.Part);

        if (Revision != other.Revision)
            return Revision.CompareTo(other.Revision);

        if (IsHistoric != other.IsHistoric)
            return other.IsHistoric.CompareTo(IsHistoric);

        // var sectionLen = Section?.Length ?? 0;
        // var otherSectionLen = other.Section?.Length ?? 0;
        // if (sectionLen != otherSectionLen)
        //     return sectionLen.CompareTo(otherSectionLen);

        var section = Section ?? string.Empty;
        var otherSection = other.Section ?? string.Empty;
        if (section != otherSection)
            return section.CompareTo(otherSection);

        return 0;
    }

    public override string ToString() =>
        ToString(null, null);

    public string ToString(string? format = null, IFormatProvider? formatProvider = null)
    {
        var sb = new StringBuilder();
        if (IsHistoric)
            sb.Append('H');
        else if (format == "P")
            sb.Append('D');

        sb.Append($"{Year:D4}.{Month:D2}.{Day:D2}.{Part:D4}.{Revision:D4}");
        if (Section is not null)
            sb.Append(Section);

        return sb.ToString();
    }

    public bool Equals(ParsedVersionString other) =>
        CompareTo(other) == 0;

    public override readonly int GetHashCode() =>
        HashCode.Combine(Year, Month, Day, Part, Revision, IsHistoric, Section);

    public static bool operator <(ParsedVersionString left, ParsedVersionString right) =>
        left.CompareTo(right) < 0;

    public static bool operator <=(ParsedVersionString left, ParsedVersionString right) =>
        left.CompareTo(right) <= 0;

    public static bool operator >(ParsedVersionString left, ParsedVersionString right) =>
        left.CompareTo(right) > 0;

    public static bool operator >=(ParsedVersionString left, ParsedVersionString right) =>
        left.CompareTo(right) >= 0;

    public sealed class JsonConverter : JsonConverter<ParsedVersionString>
    {
        public override ParsedVersionString Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options) =>
            new(reader.GetString()!);

        public override void Write(Utf8JsonWriter writer, ParsedVersionString value, JsonSerializerOptions options) =>
            writer.WriteStringValue(value.ToString());
    }
}
