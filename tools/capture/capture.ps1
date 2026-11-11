# Drives a desktop tool through a list of steps and saves a screenshot of its
# windows after each: click a control, pick a file in the "open" dialog, wait
# until the plots stop changing, save the picture.
#
# Works with Windows PowerShell 5.1 (the one every Windows has); nothing is
# installed. The steps come from a JSON file. Positions are given in the
# pixels of a reference screenshot and are scaled to the window as it is now.
#
# What it touches: only the windows of the tool's own process, found by their
# titles; the mouse, the Enter / Esc / Ctrl+V keys, and the clipboard (put
# back afterwards). What it saves: the inside of those windows as PNG files
# and a log with their titles and sizes.
#
# This file is ASCII on purpose: Windows PowerShell reads a script without a
# byte-order mark in the system code page.

param(
    [string]$Steps = "steps.json",
    [string]$Out = "capture-results"
)

$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

$source = @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class OpCapture
{
    [StructLayout(LayoutKind.Sequential)]
    private struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT { public int X; public int Y; }

    private delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumProc callback, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")]
    private static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")]
    private static extern bool GetClientRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")]
    private static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);
    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")]
    private static extern bool BringWindowToTop(IntPtr hwnd);
    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")]
    private static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    private static extern bool GetCursorPos(out POINT point);
    [DllImport("user32.dll")]
    private static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")]
    private static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")]
    private static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")]
    private static extern bool SetProcessDPIAware();

    // Real pixels on every monitor, whatever the display scaling is.
    private static void Aware()
    {
        try
        {
            IntPtr previous = SetThreadDpiAwarenessContext(new IntPtr(-4));
            if (previous == IntPtr.Zero)
            {
                SetThreadDpiAwarenessContext(new IntPtr(-3));
            }
        }
        catch (Exception)
        {
            try { SetProcessDPIAware(); } catch (Exception) { }
        }
    }

    private static string TitleOf(IntPtr hwnd)
    {
        StringBuilder text = new StringBuilder(512);
        GetWindowText(hwnd, text, text.Capacity);
        return text.ToString();
    }

    private static string ClassOf(IntPtr hwnd)
    {
        StringBuilder text = new StringBuilder(256);
        GetClassName(hwnd, text, text.Capacity);
        return text.ToString();
    }

    public static uint ProcessOf(IntPtr hwnd)
    {
        uint pid;
        GetWindowThreadProcessId(hwnd, out pid);
        return pid;
    }

    // Visible top-level windows, topmost first; of one process when pid is not 0.
    public static IntPtr[] Windows(uint pid)
    {
        Aware();
        List<IntPtr> found = new List<IntPtr>();
        EnumWindows(delegate(IntPtr hwnd, IntPtr unused)
        {
            if (!IsWindowVisible(hwnd)) { return true; }
            if (pid != 0 && ProcessOf(hwnd) != pid) { return true; }
            found.Add(hwnd);
            return true;
        }, IntPtr.Zero);
        return found.ToArray();
    }

    // The topmost visible window whose title contains the text.
    public static IntPtr Find(string part, uint pid)
    {
        foreach (IntPtr hwnd in Windows(pid))
        {
            string title = TitleOf(hwnd);
            if (title.Length > 0 && title.IndexOf(part, StringComparison.OrdinalIgnoreCase) >= 0)
            {
                return hwnd;
            }
        }
        return IntPtr.Zero;
    }

    public static string ClassName(IntPtr hwnd)
    {
        return ClassOf(hwnd);
    }

    public static string Describe(IntPtr hwnd)
    {
        int[] c = Client(hwnd);
        return "[" + ClassOf(hwnd) + "] \"" + TitleOf(hwnd) + "\" inside "
            + c[2] + "x" + c[3] + " at " + c[0] + "," + c[1];
    }

    // The inside of a window on the screen: left, top, width, height.
    public static int[] Client(IntPtr hwnd)
    {
        Aware();
        RECT rect;
        GetClientRect(hwnd, out rect);
        POINT origin = new POINT();
        origin.X = 0;
        origin.Y = 0;
        ClientToScreen(hwnd, ref origin);
        return new int[] { origin.X, origin.Y, rect.Right - rect.Left, rect.Bottom - rect.Top };
    }

    public static long Foreground()
    {
        return GetForegroundWindow().ToInt64();
    }

    public static bool Front(IntPtr hwnd)
    {
        Aware();
        if (GetForegroundWindow() == hwnd) { return true; }
        if (IsIconic(hwnd)) { ShowWindow(hwnd, 9); }
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        Thread.Sleep(150);
        if (GetForegroundWindow() != hwnd)
        {
            // Windows refuses to hand over the focus unless a key was just pressed.
            keybd_event(0x12, 0, 0, UIntPtr.Zero);
            SetForegroundWindow(hwnd);
            keybd_event(0x12, 0, 2, UIntPtr.Zero);
            Thread.Sleep(150);
        }
        return GetForegroundWindow() == hwnd;
    }

    public static void Click(int x, int y)
    {
        Aware();
        SetCursorPos(x, y);
        Thread.Sleep(60);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        Thread.Sleep(40);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }

    public static int[] Cursor()
    {
        Aware();
        POINT point;
        GetCursorPos(out point);
        return new int[] { point.X, point.Y };
    }

    public static void Key(byte key)
    {
        keybd_event(key, 0, 0, UIntPtr.Zero);
        Thread.Sleep(40);
        keybd_event(key, 0, 2, UIntPtr.Zero);
    }

    public static void Chord(byte modifier, byte key)
    {
        keybd_event(modifier, 0, 0, UIntPtr.Zero);
        Thread.Sleep(40);
        keybd_event(key, 0, 0, UIntPtr.Zero);
        Thread.Sleep(40);
        keybd_event(key, 0, 2, UIntPtr.Zero);
        Thread.Sleep(40);
        keybd_event(modifier, 0, 2, UIntPtr.Zero);
    }

    private static Bitmap Grab(int x, int y, int width, int height)
    {
        Aware();
        Bitmap bitmap = new Bitmap(width, height, PixelFormat.Format32bppRgb);
        using (Graphics graphics = Graphics.FromImage(bitmap))
        {
            graphics.CopyFromScreen(x, y, 0, 0, new Size(width, height));
        }
        return bitmap;
    }

    public static void Save(int x, int y, int width, int height, string path)
    {
        using (Bitmap bitmap = Grab(x, y, width, height))
        {
            bitmap.Save(path, ImageFormat.Png);
        }
    }

    // A fingerprint of a part of the screen: equal while nothing is redrawn.
    public static string Hash(int x, int y, int width, int height)
    {
        using (Bitmap bitmap = Grab(x, y, width, height))
        {
            Rectangle all = new Rectangle(0, 0, width, height);
            BitmapData data = bitmap.LockBits(all, ImageLockMode.ReadOnly, PixelFormat.Format32bppRgb);
            try
            {
                int count = Math.Abs(data.Stride) * height;
                byte[] bytes = new byte[count];
                Marshal.Copy(data.Scan0, bytes, 0, count);
                int stride = Math.Abs(data.Stride);
                ulong hash = 14695981039346656037UL;
                unchecked
                {
                    // every second row: enough to see a redraw, and twice as fast
                    for (int row = 0; row < height; row += 2)
                    {
                        int end = row * stride + stride;
                        for (int i = row * stride; i < end; i++)
                        {
                            hash ^= bytes[i];
                            hash *= 1099511628211UL;
                        }
                    }
                }
                return hash.ToString("x16");
            }
            finally
            {
                bitmap.UnlockBits(data);
            }
        }
    }
}
'@

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
if (-not ("OpCapture" -as [type])) {
    Add-Type -TypeDefinition $source -ReferencedAssemblies System.Drawing, System.Windows.Forms
}

$outDir = Join-Path $here $Out
if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir }
$null = New-Item -ItemType Directory -Path $outDir
$logPath = Join-Path $outDir "capture-log.txt"

function Log($text) {
    $line = "{0:HH:mm:ss.fff} {1}" -f (Get-Date), $text
    Add-Content -Path $logPath -Value $line -Encoding UTF8
    Write-Host $line
}

$script:toolPid = 0
$script:mainHandle = 0
$script:windowTitle = ""
$script:mark = $null
$script:cursor = $null
$script:failed = 0
$script:missing = @{}

function Show-ToolWindows {
    if ($script:toolPid -eq 0) { return }
    foreach ($w in [OpCapture]::Windows([uint32]$script:toolPid)) {
        Log ("    window " + [OpCapture]::Describe($w))
    }
}

# The window whose title contains $title, waiting for it up to $waitS seconds.
function Resolve-Window($title, $waitS) {
    $deadline = (Get-Date).AddSeconds($waitS)
    do {
        $h = [OpCapture]::Find($title, [uint32]$script:toolPid)
        if ($h.ToInt64() -ne 0) { return $h }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return [IntPtr]::Zero
}

# A position of the reference screenshot as a point of the screen now.
function Get-Point($hwnd, $step) {
    $ref = $plan.refs.PSObject.Properties[$step.ref].Value
    $c = [OpCapture]::Client($hwnd)
    $fx = ([double]$step.at[0] - [double]$ref.left) / [double]$ref.width
    $fy = ([double]$step.at[1] - [double]$ref.top) / [double]$ref.height
    $x = [int][Math]::Round($c[0] + $fx * $c[2])
    $y = [int][Math]::Round($c[1] + $fy * $c[3])
    return @($x, $y)
}

# The part of a window that is watched for redrawing: the plots, without the
# control panel on the right, where a pressed button changes its look.
function Get-Watch($hwnd, $step) {
    $c = [OpCapture]::Client($hwnd)
    $w = @(0.0, 0.0, 0.86, 1.0)
    if ($step.watch) { $w = $step.watch }
    $x = [int]($c[0] + [double]$w[0] * $c[2])
    $y = [int]($c[1] + [double]$w[1] * $c[3])
    $width = [int](([double]$w[2] - [double]$w[0]) * $c[2])
    $height = [int](([double]$w[3] - [double]$w[1]) * $c[3])
    return @($x, $y, $width, $height)
}

function Get-Fingerprint($hwnd, $step) {
    $r = Get-Watch $hwnd $step
    return [OpCapture]::Hash($r[0], $r[1], $r[2], $r[3])
}

# Waits until the watched part has changed since the last mark (when the step
# asks for it) and has then stayed the same for quiet_s seconds.
function Wait-Settled($hwnd, $step) {
    $changeS = 0.0
    if ($step.change_s) { $changeS = [double]$step.change_s }
    $quietS = 1.2
    if ($step.quiet_s) { $quietS = [double]$step.quiet_s }
    $maxS = 180.0
    $start = Get-Date
    $h = Get-Fingerprint $hwnd $step
    if ($changeS -gt 0 -and $script:mark) {
        while (($h -eq $script:mark) -and (((Get-Date) - $start).TotalSeconds -lt $changeS)) {
            Start-Sleep -Milliseconds 150
            $h = Get-Fingerprint $hwnd $step
        }
        $waited = ((Get-Date) - $start).TotalSeconds
        if ($h -eq $script:mark) {
            Log ("    nothing changed in {0:n1} s" -f $waited)
        } else {
            Log ("    changed after {0:n1} s" -f $waited)
        }
    }
    $last = $h
    $lastChange = Get-Date
    while ((((Get-Date) - $lastChange).TotalSeconds -lt $quietS) -and (((Get-Date) - $start).TotalSeconds -lt ($changeS + $maxS))) {
        Start-Sleep -Milliseconds 150
        $h = Get-Fingerprint $hwnd $step
        if ($h -ne $last) {
            $last = $h
            $lastChange = Get-Date
        }
    }
    Log ("    settled after {0:n1} s" -f ((Get-Date) - $start).TotalSeconds)
    $script:mark = $null
}

# Types the path of a file into the "open" dialog that a click has brought up.
function Open-File($step) {
    # One path, or several as the dialog takes them: each in quotes, a space
    # between. The name field of the dialog holds about 259 characters, so for
    # several files the folder is entered first and then the bare names.
    $folder = $null
    if ($step.paths) {
        $folder = $here
        $quoted = @()
        foreach ($one in $step.paths) {
            $null = Resolve-Path (Join-Path $here $one)
            $quoted += ('"' + $one + '"')
        }
        $path = $quoted -join " "
        if ($path.Length -gt 250) {
            throw ("STOP: {0} file names are {1} characters, more than the dialog takes at once" -f $step.paths.Count, $path.Length)
        }
    } else {
        $path = (Resolve-Path (Join-Path $here $step.path)).Path
    }
    $deadline = (Get-Date).AddSeconds(20)
    $dialog = 0
    do {
        Start-Sleep -Milliseconds 300
        $f = [OpCapture]::Foreground()
        if (($f -ne 0) -and ($f -ne $script:mainHandle)) {
            $fp = [IntPtr]$f
            $kind = [OpCapture]::ClassName($fp)
            $ownerPid = [OpCapture]::ProcessOf($fp)
            if (($kind -eq "#32770") -or ($ownerPid -eq $script:toolPid)) { $dialog = $f }
        }
    } while (($dialog -eq 0) -and ((Get-Date) -lt $deadline))
    if ($dialog -eq 0) { throw "STOP: no file dialog came up after the click on Open File" }
    Log ("    dialog " + [OpCapture]::Describe([IntPtr]$dialog))
    Start-Sleep -Milliseconds 500

    $old = $null
    try {
        if ([System.Windows.Forms.Clipboard]::ContainsText()) {
            $old = [System.Windows.Forms.Clipboard]::GetText()
        }
    } catch { }
    if ($folder) {
        # Enter on a folder only opens it; the dialog stays
        [System.Windows.Forms.Clipboard]::SetText($folder)
        Start-Sleep -Milliseconds 200
        [OpCapture]::Chord(0x11, 0x41)
        Start-Sleep -Milliseconds 100
        [OpCapture]::Chord(0x11, 0x56)
        Start-Sleep -Milliseconds 400
        [OpCapture]::Key(0x0D)
        Start-Sleep -Milliseconds 1200
        Log ("    in the folder, {0} names of {1} characters" -f $step.paths.Count, $path.Length)
    }
    [System.Windows.Forms.Clipboard]::SetText($path)
    Start-Sleep -Milliseconds 200
    # whatever is in the name field is replaced
    [OpCapture]::Chord(0x11, 0x41)
    Start-Sleep -Milliseconds 100
    [OpCapture]::Chord(0x11, 0x56)
    Start-Sleep -Milliseconds 400
    [OpCapture]::Key(0x0D)

    $deadline = (Get-Date).AddSeconds(20)
    while (([OpCapture]::Foreground() -eq $dialog) -and ((Get-Date) -lt $deadline)) {
        Start-Sleep -Milliseconds 300
    }
    if ([OpCapture]::Foreground() -eq $dialog) {
        Log "    the dialog is still open"
    } else {
        Log "    the dialog closed"
    }
    try {
        if ($old) {
            [System.Windows.Forms.Clipboard]::SetText($old)
        } else {
            [System.Windows.Forms.Clipboard]::Clear()
        }
    } catch { }
}

function Invoke-Step($step, $index) {
    $action = $step.action
    Log ("step {0}: {1}" -f $index, ($step | ConvertTo-Json -Compress))
    if ($action -eq "pause") {
        Start-Sleep -Milliseconds ([int]$step.ms)
        return
    }
    if ($action -eq "open_file") {
        Open-File $step
        return
    }
    if ($action -eq "windows") {
        Show-ToolWindows
        return
    }
    if ($action -eq "key") {
        $keys = @{ "esc" = 0x1B; "enter" = 0x0D }
        if (-not $keys.ContainsKey($step.key)) { throw ("unknown key " + $step.key) }
        [OpCapture]::Key([byte]$keys[$step.key])
        return
    }

    if ($step.window) { $script:windowTitle = $step.window }
    if (-not $script:windowTitle) { throw "the step names no window" }
    $waitS = 20
    if ($step.wait_s) { $waitS = [int]$step.wait_s }
    # a window that did not come up is not waited for again
    if ($script:missing.ContainsKey($script:windowTitle)) { $waitS = 0 }
    $hwnd = Resolve-Window $script:windowTitle $waitS
    if ($hwnd.ToInt64() -eq 0) {
        $script:missing[$script:windowTitle] = $true
        if ($script:toolPid -eq 0) {
            throw ("STOP: no window titled '" + $script:windowTitle + "' - start the tool first")
        }
        throw ("no window titled '" + $script:windowTitle + "'")
    }
    if ($script:toolPid -eq 0) {
        $script:toolPid = [OpCapture]::ProcessOf($hwnd)
        $script:mainHandle = $hwnd.ToInt64()
        Log "    the tool's windows:"
        Show-ToolWindows
    }
    # an open list would close if its window were activated again
    if (($step.front -ne $false) -and ([OpCapture]::Foreground() -ne $hwnd.ToInt64())) {
        $front = [OpCapture]::Front($hwnd)
        if (-not $front) { Log "    could not bring the window to the front" }
        Start-Sleep -Milliseconds 300
    }

    if ($action -eq "shot") {
        $c = [OpCapture]::Client($hwnd)
        $file = Join-Path $outDir ($step.file + ".png")
        [OpCapture]::Save($c[0], $c[1], $c[2], $c[3], $file)
        Log ("    saved {0}.png, {1}x{2}" -f $step.file, $c[2], $c[3])
    } elseif ($action -eq "click") {
        if ($script:cursor) {
            $now = [OpCapture]::Cursor()
            $moved = [Math]::Abs($now[0] - $script:cursor[0]) + [Math]::Abs($now[1] - $script:cursor[1])
            if ($moved -gt 60) { throw "STOP: the mouse was moved, so the run was stopped" }
        }
        if ($step.mark) { $script:mark = Get-Fingerprint $hwnd $step }
        $p = Get-Point $hwnd $step
        [OpCapture]::Click($p[0], $p[1])
        $script:cursor = $p
        Log ("    clicked '{0}' at {1},{2}" -f $step.name, $p[0], $p[1])
    } elseif ($action -eq "settle") {
        Wait-Settled $hwnd $step
    } else {
        throw ("unknown step '" + $action + "'")
    }
}

$stepsPath = Join-Path $here $Steps
$plan = Get-Content -Raw -Path $stepsPath -Encoding UTF8 | ConvertFrom-Json

Log ("capture of '{0}', {1} steps" -f $Steps, $plan.steps.Count)
Log ("Windows {0}, PowerShell {1}" -f [Environment]::OSVersion.Version, $PSVersionTable.PSVersion)
Log ("primary screen as this process sees it: {0}" -f [System.Windows.Forms.Screen]::PrimaryScreen.Bounds)

Write-Host ""
Write-Host "The mouse and the keyboard are used by the script now."
Write-Host "Do not touch them until it says it is done. Moving the mouse stops it."
Write-Host "Starting in 5 seconds..."
Start-Sleep -Seconds 5

$index = 0
foreach ($step in $plan.steps) {
    $index++
    try {
        Invoke-Step $step $index
    } catch {
        $script:failed++
        Log ("    FAILED: " + $_.Exception.Message)
        if ($_.Exception.Message -like "STOP*") { break }
    }
}

Log "the tool's windows at the end:"
Show-ToolWindows
Log ("done: {0} steps, {1} failed" -f $index, $script:failed)
try { [Console]::Beep(880, 250); [Console]::Beep(1175, 350) } catch { }

$zip = Join-Path $here ($Out + ".zip")
if (Test-Path $zip) { Remove-Item -Force $zip }
try {
    Compress-Archive -Path $outDir -DestinationPath $zip
    Write-Host ""
    Write-Host ("Done. Please send back this file: " + $zip)
} catch {
    Write-Host ""
    Write-Host ("Done. Please send back this folder: " + $outDir)
}
