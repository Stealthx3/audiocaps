// SPDX-License-Identifier: LGPL-2.1-or-later
// AudioCaps from .NET with P/Invoke (no binding package; bundle libaudiocaps.so.1 with the app).
// Uses the watch's thread mode: the callback runs on the library's event thread.
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

static class Native
{
    const string Lib = "audiocaps";

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void WatchCallback(IntPtr user, IntPtr snapshot, IntPtr diffJson);

    [DllImport(Lib)] public static extern uint ac_abi_version();
    [DllImport(Lib)] public static extern IntPtr ac_schema_version();
    [DllImport(Lib)] public static extern IntPtr ac_last_error();
    [DllImport(Lib)] public static extern void ac_string_free(IntPtr s);
    [DllImport(Lib)] public static extern IntPtr ac_context_new();
    [DllImport(Lib)] public static extern IntPtr ac_context_new_fixture(byte[] dir);
    [DllImport(Lib)] public static extern void ac_context_free(IntPtr ctx);
    [DllImport(Lib)] public static extern IntPtr ac_probe(IntPtr ctx, byte[] scope, byte[] options);
    [DllImport(Lib)] public static extern void ac_snapshot_free(IntPtr snap);
    [DllImport(Lib)] public static extern ulong ac_snapshot_generation(IntPtr snap);
    [DllImport(Lib)] public static extern UIntPtr ac_snapshot_endpoint_count(IntPtr snap);
    [DllImport(Lib)] public static extern IntPtr ac_snapshot_endpoint_id(IntPtr snap, UIntPtr index);
    [DllImport(Lib)] public static extern IntPtr ac_snapshot_findings_json(IntPtr snap);
    [DllImport(Lib)] public static extern int ac_can_play(IntPtr snap, IntPtr id, byte[] path, byte[] format, out IntPtr reason);
    [DllImport(Lib)] public static extern IntPtr ac_watch_new(IntPtr ctx, byte[] scope, byte[] options, uint mode, WatchCallback cb, IntPtr user);
    [DllImport(Lib)] public static extern void ac_watch_free(IntPtr w);

    public const uint AC_WATCH_THREAD = 1;

    public static byte[] Utf8(string s) => s == null ? null : Encoding.UTF8.GetBytes(s + "\0");

    public static string Str(IntPtr p)
    {
        if (p == IntPtr.Zero) return null;
        int n = 0;
        while (Marshal.ReadByte(p, n) != 0) n++;
        var b = new byte[n];
        Marshal.Copy(p, b, 0, n);
        return Encoding.UTF8.GetString(b);
    }

    public static int Len(IntPtr p) { int n = 0; while (Marshal.ReadByte(p, n) != 0) n++; return n; }

    /// Copies an owned string and frees it.
    public static string Take(IntPtr p) { try { return Str(p); } finally { if (p != IntPtr.Zero) ac_string_free(p); } }
}

static class Program
{
    static readonly string[] Paths = { "alsa_direct", "pulse_audio", "pipe_wire", "jack" };
    static readonly string[] Labels = { "lpcm6", "ac3" };
    static readonly string[] Formats = { "{\"channels\":6,\"rate_hz\":48000,\"bits\":24}", "{\"codec\":\"ac3\"}" };
    static readonly string[] Answer = { "error", "unknown", "no", "yes" };

    static IntPtr Check(IntPtr h, string what)
    {
        if (h == IntPtr.Zero) throw new Exception(what + ": " + Native.Str(Native.ac_last_error()));
        return h;
    }

    static int Main(string[] args)
    {
        var stdout = new System.IO.StreamWriter(Console.OpenStandardOutput(), new UTF8Encoding(false)) { AutoFlush = true, NewLine = "\n" };
        Console.SetOut(stdout);
        Console.WriteLine($"abi {Native.ac_abi_version()}\nschema {Native.Str(Native.ac_schema_version())}");
        IntPtr ctx = Check(args.Length > 0 ? Native.ac_context_new_fixture(Native.Utf8(args[0])) : Native.ac_context_new(), "context");
        byte[] options = Native.Utf8("{\"deadline_ms\":0}");
        IntPtr snap = Check(Native.ac_probe(ctx, null, options), "probe");
        ulong n = (ulong)Native.ac_snapshot_endpoint_count(snap);
        Console.WriteLine($"generation {Native.ac_snapshot_generation(snap)}\nendpoints {n}");
        for (ulong i = 0; i < n; i++)
        {
            IntPtr id = Native.ac_snapshot_endpoint_id(snap, (UIntPtr)i);
            foreach (var path in Paths)
                for (int f = 0; f < 2; f++)
                {
                    int a = Native.ac_can_play(snap, id, Native.Utf8(path), Native.Utf8(Formats[f]), out IntPtr reason);
                    Console.WriteLine($"{Native.Str(id)} {path} {Labels[f]} {Answer[a + 2]} {Native.Take(reason) ?? "-"}");
                }
        }
        Console.WriteLine($"findings_bytes {Encoding.UTF8.GetByteCount(Native.Take(Native.ac_snapshot_findings_json(snap)))}");
        Native.ac_snapshot_free(snap);

        using (var got = new ManualResetEventSlim())
        {
            // Keep the delegate alive while the watch runs.
            Native.WatchCallback cb = (user, s, diff) =>
            {
                Console.WriteLine($"watch generation {Native.ac_snapshot_generation(s)} endpoints {(ulong)Native.ac_snapshot_endpoint_count(s)} diff_bytes {Native.Len(diff)}");
                got.Set();
            };
            IntPtr w = Check(Native.ac_watch_new(ctx, null, options, Native.AC_WATCH_THREAD, cb, IntPtr.Zero), "watch");
            got.Wait(TimeSpan.FromSeconds(10));
            Native.ac_watch_free(w);
            GC.KeepAlive(cb);
        }
        Native.ac_context_free(ctx);
        return 0;
    }
}
