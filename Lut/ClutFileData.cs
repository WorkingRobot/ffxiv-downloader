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

    public void FilterIntervals()
    {
        List<int> removals = [];
        List<(long Start, long End)> intervals = [];
        var comparer = Comparer<(long Start, long End)>.Create((i, v) => i.Start.CompareTo(v.Start));
        bool IsMasked(long start, long end) =>
            intervals.Any(i => i.Start <= start && i.End >= end);
        bool AddInterval(long start, long end)
        {
            if (IsMasked(start, end))
                return false;

            // Merge intervals together if needed
            var idx = intervals.BinarySearch((start, end), comparer);
            if (idx < 0)
                idx = ~idx;

            if (idx > 0 && intervals[idx - 1].End >= start)
            {
                idx--;
                start = Math.Min(start, intervals[idx].Start);
                end = Math.Max(end, intervals[idx].End);
                intervals.RemoveAt(idx);
            }

            var mergeStart = idx;
            var mergeCount = 0;
            while (idx < intervals.Count && intervals[idx].Start <= end)
            {
                start = Math.Min(start, intervals[idx].Start);
                end = Math.Max(end, intervals[idx].End);
                idx++;
                mergeCount++;
            }

            if (mergeCount > 0)
                intervals.RemoveRange(mergeStart, mergeCount);

            intervals.Insert(mergeStart, (start, end));
            return true;
        }

        for (var i = Data.Count - 1; i >= 0; --i)
        {
            if (!AddInterval(Data[i].Offset, Data[i].End))
                removals.Add(i);
        }

        RemoveMultiple(Data, ((IEnumerable<int>)removals).Reverse());
    }

    private static Comparer<ClutDataRef> StartComparer { get; } = Comparer<ClutDataRef>.Create((a, b) => a.Offset.CompareTo(b.Offset));
    private static Comparer<ClutDataRef> EndComparer { get; } = Comparer<ClutDataRef>.Create((a, b) => a.End.CompareTo(b.End));

    private static void AdjustInPlace(List<ClutDataRef> intervals, ClutDataRef newInterval)
    {
        // First, find the first index where an interval might overlap newInterval.
        var n = intervals.Count;
        //var startIndex = 0;
        //while (startIndex < n && intervals[startIndex].End <= newInterval.Offset)
        //    startIndex++;
        int startIndex;
        {
            var dummyForEnd = new ClutDataRef
            {
                Offset = newInterval.Offset,
                Length = 1
            };
            startIndex = intervals.BinarySearch(dummyForEnd, EndComparer);
            if (startIndex < 0)
                startIndex = ~startIndex;
        }

        // We'll also use a temporary list for any new intervals created by splitting.
        List<ClutDataRef> splits = [];

        // Process overlapping intervals starting at startIndex.
        var i = startIndex;
        while (i < intervals.Count && intervals[i].Offset < newInterval.End)
        {
            var curr = intervals[i];

            // Case 1: Current interval extends before newInterval: create left split.
            if (curr.Offset < newInterval.Offset)
            {
                // Replace the current interval's end with newInterval.Start.
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

        // Now, insert the new interval into the proper position.
        // (Assuming you want to keep it alongside the other intervals.)
        //var insertIndex = startIndex;
        // Use binary search manually if needed.
        //while (insertIndex < intervals.Count && intervals[insertIndex].Offset < newInterval.Offset)
        //    insertIndex++;
        var insertIndex = intervals.BinarySearch(newInterval, StartComparer);
        if (insertIndex < 0)
            insertIndex = ~insertIndex;
        intervals.Insert(insertIndex, newInterval);

        // Insert any splits that were created.
        foreach (var s in splits)
        {
            // Find the insertion index for each split piece.
            var idx = intervals.BinarySearch(s, StartComparer);
            if (idx < 0)
                idx = ~idx;
            else
            {
                if (intervals[idx] == s)
                {
                    Log.Warn("Duplicate split interval");
                    continue;
                }
            }
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
