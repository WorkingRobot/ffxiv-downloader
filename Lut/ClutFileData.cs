using DotNext;
using FFXIVDownloader.Thaliak;
using System.Collections.Frozen;
using System.Diagnostics;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace FFXIVDownloader.Lut;

public sealed class ClutFileData
{
    public List<ClutDataRef> Data { get; set; }

    public ClutFileData()
    {
        Data = [];
    }

    public ClutFileData(BinaryReader reader, ReadOnlySpan<ParsedVersionString> patchMap)
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
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public void Write(BinaryWriter writer, FrozenDictionary<ParsedVersionString, int> patchMap)
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

    private static void AdjustInPlace(List<ClutDataRef> intervals, ClutDataRef newInterval)
    {
        var startIndex = intervals.BinarySearch(new ClutDataRef
            {
                Offset = newInterval.Offset,
                Length = 1
            }, EndComparer);
            if (startIndex < 0)
                startIndex = ~startIndex;

        List<ClutDataRef> splits = [];

        var i = startIndex;
        while (i < intervals.Count && intervals[i].Offset < newInterval.End)
        {
            var curr = intervals[i];

            // Case 1: Current interval extends before newInterval: create left split.
            if (curr.Offset < newInterval.Offset)
            {
                // Replace the current interval's end with newInterval.Offset.
                intervals[i] = ClutDataRef.FromSliceInterval(in curr, curr.Offset, newInterval.Offset);
                if (curr.End > newInterval.End)
                {
                    // If curr also extends beyond newInterval, we need a right split.
                    splits.Add(ClutDataRef.FromSliceInterval(in curr, newInterval.End, curr.End));
                }
            }
            // Case 2: Current interval starts within newInterval.
            else
            {
                if (curr.End <= newInterval.End)
                {
                    // The entire interval is swallowed by newInterval, remove it.
                    intervals.RemoveAt(i);
                    i--;
                }
                else
                {
                    // Part of the interval is beyond newInterval: adjust its start.
                    intervals[i] = ClutDataRef.FromSliceInterval(in curr, newInterval.End, curr.End);
                }
            }
            i++;
        }

        var insertIndex = intervals.BinarySearch(newInterval, StartComparer);
        if (insertIndex < 0)
            insertIndex = ~insertIndex;
        intervals.Insert(insertIndex, newInterval);

        // Insert any splits that were created.
        foreach (var s in splits)
        {
            var idx = intervals.BinarySearch(s, StartComparer);
            if (idx < 0)
                idx = ~idx;
            intervals.Insert(idx, s);
        }
    }

    public void RemoveOverlaps()
    {
        (var intervals, Data) = (Data, []);
        foreach (var interval in intervals)
            AdjustInPlace(Data, interval);
        VerifyOverlaps("Removal Overlap!");
    }

    // Prints if there is any overlap between any blocks
    [Conditional("DEBUG")]
    public void VerifyOverlaps(string prefix)
    {
        Debug.Assert(Data.Order(EndComparer).SequenceEqual(Data), "Intervals are unordered by end");
        Debug.Assert(Data.Order(StartComparer).SequenceEqual(Data), "Intervals are unordered by start");
        ClutDataRef? p = null;
        foreach(var curr in Data)
        {
            if (p is { } prev && prev.End > curr.Offset)
                Log.Warn($"{prefix} {prev.Offset}; {prev.Length} ({prev.Type}) and {curr.Offset}; {curr.Length} ({curr.Type})");
            p = curr;
        }
    }

    public void WipeZeros()
    {
        var removals = new List<int>();
        for (var i = 0; i < Data.Count; i++)
        {
            if (Data[i].Type == ClutDataRef.RefType.Zero)
                removals.Add(i);
        }

        RemoveMultiple(Data, removals);
        VerifyOverlaps("Zeros Overlap!");
    }

    // Indices must be ordered in ascending order
    private static void RemoveMultiple<T>(List<T> list, IEnumerable<int> indices)
    {
        var data = CollectionsMarshal.AsSpan(list);
        var current = 0;
        var copyFrom = 0;
        foreach (var idx in indices)
        {
            if (idx < copyFrom)
                continue;

            var copyLength = idx - copyFrom;
            if (copyFrom != current && copyLength > 0)
                data.Slice(copyFrom, copyLength)
                    .CopyTo(data.Slice(current, copyLength));
            current += copyLength;
            copyFrom = idx + 1;
        }

        var tailLength = list.Count - copyFrom;
        if (tailLength > 0)
        {
            data.Slice(copyFrom, tailLength)
                .CopyTo(data.Slice(current, tailLength));
        }
        current += tailLength;

        list.RemoveRange(current, list.Count - current);
    }
}
