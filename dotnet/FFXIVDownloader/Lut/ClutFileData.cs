using FFXIVDownloader.Thaliak;
using System.Collections.Frozen;
using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace FFXIVDownloader.Lut;

public sealed class ClutFileData
{
    public List<ClutDataRef> Data { get; set; }
    private int LastOptimizationSize { get; set; }

    public ClutFileData()
    {
        Data = [];
        LastOptimizationSize = 0;
    }

    public ClutFileData(BinaryReader reader, ReadOnlySpan<PatchVersion> patchMap)
    {
        var dataSize = reader.ReadInt32();
        Data = new(dataSize);
        long lastOffset = 0;
        for (var i = 0; i < dataSize; ++i)
            Data.Add(new(reader, patchMap, ref lastOffset));
        lastOffset = 0;

        var data = CollectionsMarshal.AsSpan(Data);
        foreach (ref var item in data)
            item.ReadOffset(reader, ref lastOffset);
        foreach (ref var item in data)
            item.ReadLength(reader);

        LastOptimizationSize = dataSize;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public void Write(BinaryWriter writer, FrozenDictionary<PatchVersion, int> patchMap)
    {
        writer.Write(Data.Count);
        long lastOffset = 0;
        foreach (var data in Data)
            data.Write(writer, patchMap, ref lastOffset);
        lastOffset = 0;
        foreach (var data in Data)
            data.WriteOffset(writer, ref lastOffset);
        foreach (var data in Data)
            data.WriteLength(writer);
    }

    private static Comparer<ClutDataRef> StartComparer { get; } = Comparer<ClutDataRef>.Create((a, b) => a.Offset.CompareTo(b.Offset));
    private static Comparer<ClutDataRef> EndComparer { get; } = Comparer<ClutDataRef>.Create((a, b) => a.End.CompareTo(b.End));

    /// <summary>
    /// Applies a new interval (newSegment) to the list of segments, removing or splitting any overlapping intervals.
    /// The input list <paramref name="segments"/> is expected to be sorted by start offset for correctness.
    /// </summary>
    /// <param name="segments">Sorted list of intervals (by start offset).</param>
    /// <param name="newSegment">The new interval to overlay.</param>
    private static void ApplyOverlay(List<ClutDataRef> segments, ClutDataRef newSegment)
    {
        var newStart = newSegment.Offset;
        var newEnd = newSegment.End;

        // Find the insertion index for the new interval using binary search.
        var index = segments.BinarySearch(newSegment, StartComparer);
        if (index < 0)
            index = ~index; // If not found, get the index where it should be inserted.

        ClutDataRef? rightSegment = null;

        // Check if the interval immediately to the left overlaps with the new interval.
        if (index > 0)
        {
            var prev = segments[index - 1];
            // If previous interval ends after newStart, there is an overlap.
            if (prev.End > newStart)
            {
                // If previous interval starts before newStart, preserve its left portion.
                if (prev.Offset < newStart)
                    segments[index - 1] = ClutDataRef.FromSliceInterval(in prev, prev.Offset, newStart);

                // If previous interval extends beyond newEnd, split it and keep the right piece.
                if (prev.End > newEnd)
                    rightSegment = ClutDataRef.FromSliceInterval(in prev, newEnd, prev.End);
            }
        }

        // Remove or adjust intervals that are overlapped by the new interval.
        var removeIndex = index;
        while (removeIndex < segments.Count && segments[removeIndex].Offset < newEnd)
        {
            var curr = segments[removeIndex];
            // If the current interval ends after newEnd, adjust its start to newEnd.
            if (curr.End > newEnd)
            {
                segments[removeIndex] = ClutDataRef.FromSliceInterval(in curr, newEnd, curr.End);
                break; // No further intervals will overlap.
            }
            removeIndex++;
        }

        // Remove all intervals that are fully overlapped by the new interval.
        if (removeIndex > index)
            segments.RemoveRange(index, removeIndex - index);

        // Insert the new interval at the calculated index.
        segments.Insert(index, newSegment);

        // If a right segment was split off, insert it immediately after the new interval.
        if (rightSegment != null)
            segments.Insert(index + 1, rightSegment.Value);
    }

    public void RemoveOverlaps(string name)
    {
        if (LastOptimizationSize == Data.Count)
            return;

        (var intervals, Data) = (Data, []);
        foreach (var interval in intervals)
            ApplyOverlay(Data, interval);
        VerifyOverlaps("Removal Overlap!");

        LastOptimizationSize = Data.Count;
    }

    // Prints if there is any overlap between any blocks
    //[Conditional("DEBUG")]
    public void VerifyOverlaps(string prefix)
    {
        Debug.Assert(Data.Order(EndComparer).SequenceEqual(Data), "Intervals are unordered by end");
        Debug.Assert(Data.Order(StartComparer).SequenceEqual(Data), "Intervals are unordered by start");
        ClutDataRef? p = null;
        foreach (var curr in Data)
        {
            if (p is { } prev && prev.End > curr.Offset)
                Log.Warn($"{prefix} {prev.Offset}; {prev.Length} ({prev.Type}) and {curr.Offset}; {curr.Length} ({curr.Type})");
            p = curr;
        }
    }
}
