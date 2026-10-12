# SWCH product component. It never reads other browser profiles or solves CAPTCHA.
# stdin: one UTF-8 JSON request; stdout: one compact JSON result. Close stdin after
# writing the request. This helper captures only a foreground Epic launcher client.
# Snapshot request: {"mode":"snapshot","hwnd":"123","pid":123}
# Click request: {"mode":"click","hwnd":"123","pid":123,
#   "rect":{"x":0,"y":0,"width":100,"height":100},"image_hash":"sha256 hex",
#   "last_input_tick":123,"button":{"text":"Get",
#   "rect":{"x":10,"y":20,"width":25,"height":15}}}
# The controller must verify the selected account, exact game and zero price;
# this helper additionally revalidates pixels, input activity and the OCR label.
# availability and self_test modes never capture a real window or send input.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)

function Write-Result($value) {
    [Console]::Out.WriteLine(($value | ConvertTo-Json -Depth 12 -Compress))
}

try {
    $requestText = [Console]::In.ReadToEnd()
    if ($requestText.Length -eq 0 -or $requestText.Length -gt 65536) { throw 'invalid_request' }
    $request = $requestText | ConvertFrom-Json
    if ($request.mode -notin @('availability', 'snapshot', 'click', 'self_test')) { throw 'invalid_mode' }

    Add-Type -AssemblyName System.Drawing
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType=WindowsRuntime] | Out-Null
    [Windows.Media.Ocr.OcrResult, Windows.Foundation, ContentType=WindowsRuntime] | Out-Null
    [Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType=WindowsRuntime] | Out-Null
    [Windows.Graphics.Imaging.SoftwareBitmap, Windows.Foundation, ContentType=WindowsRuntime] | Out-Null
    [Windows.Storage.Streams.InMemoryRandomAccessStream, Windows.Foundation, ContentType=WindowsRuntime] | Out-Null
    [Windows.Storage.Streams.DataWriter, Windows.Foundation, ContentType=WindowsRuntime] | Out-Null
    [Windows.Globalization.Language, Windows.Globalization, ContentType=WindowsRuntime] | Out-Null

    $language = [Windows.Globalization.Language]::new('en-US')
    $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($language)
    if ($null -eq $engine) { throw 'english_ocr_unavailable' }
    $script:awaitMethod = [System.WindowsRuntimeSystemExtensions].GetMethods() |
        Where-Object { $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and
            $_.GetParameters().Count -eq 1 -and
            $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } |
        Select-Object -First 1
    if ($null -eq $script:awaitMethod) { throw 'winrt_async_unavailable' }

    function Resolve-Operation($operation, [Type]$resultType) {
        $task = $script:awaitMethod.MakeGenericMethod($resultType).Invoke($null, @($operation))
        if (-not $task.Wait(10000)) { throw 'ocr_timeout' }
        return $task.GetAwaiter().GetResult()
    }

    function Read-Words([System.Drawing.Bitmap]$bitmap) {
        $memory = New-Object System.IO.MemoryStream
        $stream = $null
        $writer = $null
        $software = $null
        try {
            $bitmap.Save($memory, [System.Drawing.Imaging.ImageFormat]::Bmp)
            $stream = [Windows.Storage.Streams.InMemoryRandomAccessStream]::new()
            $writer = [Windows.Storage.Streams.DataWriter]::new($stream)
            $writer.WriteBytes($memory.ToArray())
            $stored = Resolve-Operation ($writer.StoreAsync()) ([uint32])
            $writer.DetachStream() | Out-Null
            $stream.Seek(0)
            $decoder = Resolve-Operation ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
            $software = Resolve-Operation ($decoder.GetSoftwareBitmapAsync(
                [Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8,
                [Windows.Graphics.Imaging.BitmapAlphaMode]::Premultiplied)) ([Windows.Graphics.Imaging.SoftwareBitmap])
            $result = Resolve-Operation ($engine.RecognizeAsync($software)) ([Windows.Media.Ocr.OcrResult])
            $words = @()
            $lineNumber = 0
            foreach ($line in $result.Lines) {
                foreach ($word in $line.Words) {
                    $box = $word.BoundingRect
                    $words += [pscustomobject]@{
                        text = [string]$word.Text; x = [double]$box.X; y = [double]$box.Y
                        width = [double]$box.Width; height = [double]$box.Height; line = $lineNumber
                    }
                }
                $lineNumber++
            }
            return ,$words
        } finally {
            if ($null -ne $software) { $software.Dispose() }
            if ($null -ne $writer) { $writer.Dispose() }
            if ($null -ne $stream) { $stream.Dispose() }
            $memory.Dispose()
        }
    }

    if ($request.mode -eq 'availability') {
        Write-Result @{ ok = $true; version = 1; available = $true; language = 'en-US';
            max_image_dimension = [Windows.Media.Ocr.OcrEngine]::MaxImageDimension }
        exit 0
    }

    if ($request.mode -eq 'self_test') {
        $bitmap = New-Object System.Drawing.Bitmap(800, 220)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $font = New-Object System.Drawing.Font('Arial', 28)
        try {
            $graphics.Clear([System.Drawing.Color]::White)
            $graphics.DrawString('Get', $font, [System.Drawing.Brushes]::Black, 20, 15)
            $graphics.DrawString('Add to library', $font, [System.Drawing.Brushes]::Black, 20, 85)
            $graphics.DrawString('Place order', $font, [System.Drawing.Brushes]::Black, 20, 155)
            $words = Read-Words $bitmap
            Write-Result @{ ok = $true; version = 1; synthetic = $true; width = 800; height = 220; words = @($words) }
        } finally { $font.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
        exit 0
    }

    Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

namespace SwchEpicVision {
    public sealed class Capture : IDisposable {
        public Bitmap Bitmap;
        public int X, Y, Width, Height;
        public uint InputTick;
        public string Hash;
        public void Dispose() { if (Bitmap != null) Bitmap.Dispose(); }
    }
    public static class Native {
        [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
        [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
        [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO { public uint cbSize, dwTime; }
        [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT {
            public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo;
        }
        [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }
        delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr data);
        [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")] static extern bool IsWindow(IntPtr hwnd);
        [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
        [DllImport("user32.dll")] static extern bool IsIconic(IntPtr hwnd);
        [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr hwnd, out RECT rect);
        [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
        [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);
        [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
        [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO info);
        [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
        [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);
        [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT point);
        [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
        [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
        [DllImport("user32.dll", SetLastError=true)] static extern uint SendInput(uint count, INPUT[] input, int size);
        [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
        [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr value);
        [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
        [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, uint attribute, out int value, int size);

        public static void InitDpi() {
            try { SetProcessDpiAwarenessContext(new IntPtr(-4)); }
            catch (EntryPointNotFoundException) { SetProcessDPIAware(); }
            try { SetThreadDpiAwarenessContext(new IntPtr(-4)); }
            catch (EntryPointNotFoundException) { }
        }
        static void NoHeldInput() {
            for (int key = 1; key < 255; key++) {
                if ((GetAsyncKeyState(key) & 0x8000) != 0)
                    throw new InvalidOperationException("user_input_active");
            }
        }
        public static uint InputTick() {
            LASTINPUTINFO input = new LASTINPUTINFO(); input.cbSize = (uint)Marshal.SizeOf(input);
            if (!GetLastInputInfo(ref input)) throw new InvalidOperationException("input_state_unavailable");
            return input.dwTime;
        }
        static RECT BoundWindow(IntPtr hwnd, uint expectedPid) {
            if (hwnd == IntPtr.Zero || !IsWindow(hwnd) || !IsWindowVisible(hwnd) || IsIconic(hwnd))
                throw new InvalidOperationException("window_unavailable");
            if (GetForegroundWindow() != hwnd || GetAncestor(hwnd, 2) != hwnd)
                throw new InvalidOperationException("window_not_foreground");
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (pid != expectedPid) throw new InvalidOperationException("window_process_changed");
            using (Process process = Process.GetProcessById((int)pid)) {
                if (!String.Equals(Path.GetFileName(process.MainModule.FileName), "EpicGamesLauncher.exe", StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException("unexpected_process");
            }
            RECT client; POINT origin = new POINT();
            if (!GetClientRect(hwnd, out client) || !ClientToScreen(hwnd, ref origin))
                throw new InvalidOperationException("window_geometry_unavailable");
            int width = client.Right - client.Left, height = client.Bottom - client.Top;
            if (width < 400 || height < 300 || width > 10000 || height > 10000 || (long)width * height > 24000000)
                throw new InvalidOperationException("invalid_window_size");
            RECT screen = new RECT { Left = origin.X, Top = origin.Y, Right = origin.X + width, Bottom = origin.Y + height };
            int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
            if (screen.Left < vx || screen.Top < vy || screen.Right > vx + vw || screen.Bottom > vy + vh)
                throw new InvalidOperationException("window_outside_screen");
            bool targetSeen = false, occluded = false;
            EnumWindows(delegate(IntPtr other, IntPtr unused) {
                if (other == hwnd) { targetSeen = true; return false; }
                if (!IsWindowVisible(other) || IsIconic(other)) return true;
                int cloaked;
                if (DwmGetWindowAttribute(other, 14, out cloaked, 4) == 0 && cloaked != 0) return true;
                RECT otherRect;
                if (GetWindowRect(other, out otherRect) && otherRect.Left < screen.Right && otherRect.Right > screen.Left &&
                    otherRect.Top < screen.Bottom && otherRect.Bottom > screen.Top) occluded = true;
                return true;
            }, IntPtr.Zero);
            if (!targetSeen || occluded) throw new InvalidOperationException("window_occluded");
            return screen;
        }
        public static Capture Snapshot(long handle, uint pid) {
            IntPtr hwnd = new IntPtr(handle);
            NoHeldInput();
            uint tick = InputTick(); RECT rect = BoundWindow(hwnd, pid);
            Bitmap bitmap = new Bitmap(rect.Right - rect.Left, rect.Bottom - rect.Top, PixelFormat.Format32bppArgb);
            try {
                using (Graphics graphics = Graphics.FromImage(bitmap)) {
                    graphics.CopyFromScreen(rect.Left, rect.Top, 0, 0, bitmap.Size, CopyPixelOperation.SourceCopy);
                }
                RECT after = BoundWindow(hwnd, pid);
                if (after.Left != rect.Left || after.Top != rect.Top || after.Right != rect.Right || after.Bottom != rect.Bottom)
                    throw new InvalidOperationException("window_geometry_changed");
                if (InputTick() != tick) throw new InvalidOperationException("user_input_changed");
                BitmapData pixels = bitmap.LockBits(new Rectangle(0, 0, bitmap.Width, bitmap.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
                string hash;
                try {
                    byte[] bytes = new byte[pixels.Stride * bitmap.Height];
                    Marshal.Copy(pixels.Scan0, bytes, 0, bytes.Length);
                    using (SHA256 sha = SHA256.Create()) { hash = BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", "").ToLowerInvariant(); }
                } finally { bitmap.UnlockBits(pixels); }
                return new Capture { Bitmap = bitmap, X = rect.Left, Y = rect.Top, Width = bitmap.Width, Height = bitmap.Height, InputTick = tick, Hash = hash };
            } catch { bitmap.Dispose(); throw; }
        }
        public static void Validate(long handle, uint pid, int x, int y, int width, int height, uint tick) {
            NoHeldInput();
            RECT rect = BoundWindow(new IntPtr(handle), pid);
            if (rect.Left != x || rect.Top != y || rect.Right - rect.Left != width || rect.Bottom - rect.Top != height)
                throw new InvalidOperationException("window_geometry_changed");
            if (InputTick() != tick) throw new InvalidOperationException("user_input_changed");
        }
        public static uint Click(long handle, uint pid, int x, int y, int width, int height, uint tick, string imageHash, int localX, int localY) {
            Validate(handle, pid, x, y, width, height, tick);
            if (localX < 0 || localY < 0 || localX >= width || localY >= height) throw new InvalidOperationException("invalid_click_point");
            POINT point = new POINT { X = x + localX, Y = y + localY };
            if (GetAncestor(WindowFromPoint(point), 2) != new IntPtr(handle)) throw new InvalidOperationException("click_point_occluded");
            int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
            if (vw <= 1 || vh <= 1) throw new InvalidOperationException("screen_geometry_unavailable");
            INPUT[] actions = new INPUT[3];
            actions[0].mi.dx = (int)Math.Round((point.X - vx) * 65535.0 / (vw - 1));
            actions[0].mi.dy = (int)Math.Round((point.Y - vy) * 65535.0 / (vh - 1));
            actions[0].mi.dwFlags = 0x8000 | 0x4000 | 0x0001;
            actions[1].mi.dwFlags = 0x0002; actions[2].mi.dwFlags = 0x0004;
            // OCR is asynchronous. Recheck the pixels once more after recognition,
            // immediately before input, so a page transition invalidates the click.
            using (Capture fresh = Snapshot(handle, pid)) {
                if (fresh.InputTick != tick) throw new InvalidOperationException("user_input_changed");
                if (!String.Equals(fresh.Hash, imageHash, StringComparison.Ordinal))
                    throw new InvalidOperationException("snapshot_changed");
            }
            Validate(handle, pid, x, y, width, height, tick);
            // Give all three events one current timestamp. Mouse move, down and
            // up are submitted together, never across helper invocations.
            uint sentTick = unchecked((uint)Environment.TickCount);
            for (int i = 0; i < actions.Length; i++) actions[i].mi.time = sentTick;
            uint count = SendInput(3, actions, Marshal.SizeOf(typeof(INPUT)));
            if (count != 3) {
                // Release a possibly pressed button if only part of the input was accepted.
                INPUT[] release = new INPUT[1]; release[0].mi.dwFlags = 0x0004;
                SendInput(1, release, Marshal.SizeOf(typeof(INPUT)));
                throw new InvalidOperationException("input_not_accepted");
            }
            Stopwatch delivery = Stopwatch.StartNew();
            uint deliveredTick = InputTick();
            while (deliveredTick == tick && delivery.ElapsedMilliseconds < 40) {
                System.Threading.Thread.Sleep(2);
                deliveredTick = InputTick();
            }
            if (deliveredTick != tick && deliveredTick != sentTick)
                throw new InvalidOperationException("user_input_changed");
            return deliveredTick;
        }
    }
}
'@
    [SwchEpicVision.Native]::InitDpi()

    [long]$handle = 0
    [uint32]$processId = 0
    if (-not [long]::TryParse([string]$request.hwnd, [ref]$handle) -or $handle -le 0 -or
        -not [uint32]::TryParse([string]$request.pid, [ref]$processId) -or $processId -eq 0) { throw 'invalid_window_binding' }
    $capture = [SwchEpicVision.Native]::Snapshot($handle, $processId)
    try {
        $words = @(Read-Words $capture.Bitmap)
        [SwchEpicVision.Native]::Validate($handle, $processId, $capture.X, $capture.Y, $capture.Width, $capture.Height, $capture.InputTick)
        $rect = @{ x = $capture.X; y = $capture.Y; width = $capture.Width; height = $capture.Height }
        if ($request.mode -eq 'snapshot') {
            Write-Result @{ ok = $true; version = 1; hwnd = [string]$handle; pid = $processId;
                rect = $rect; width = $capture.Width; height = $capture.Height;
                image_hash = $capture.Hash; last_input_tick = $capture.InputTick; words = @($words) }
            exit 0
        }

        [uint32]$expectedTick = 0
        if (-not [uint32]::TryParse([string]$request.last_input_tick, [ref]$expectedTick) -or
            $capture.InputTick -ne $expectedTick) { throw 'user_input_changed' }
        if ([string]$request.image_hash -cne $capture.Hash) { throw 'snapshot_changed' }
        if ($null -eq $request.rect -or $request.rect.x -ne $capture.X -or $request.rect.y -ne $capture.Y -or
            $request.rect.width -ne $capture.Width -or $request.rect.height -ne $capture.Height) { throw 'window_geometry_changed' }

        $label = (([string]$request.button.text).Trim().ToLowerInvariant() -replace '\s+', ' ')
        if ($label -notin @('get', 'add to library', 'place order')) { throw 'button_not_allowed' }
        $allText = (($words | ForEach-Object { $_.text }) -join ' ').ToLowerInvariant()
        if ($allText -match 'captcha|verify\s+you\s+are\s+human|security\s+check|one\s+more\s+step') { throw 'security_check' }
        $buttonRect = $request.button.rect
        if ($null -ne $buttonRect) {
            foreach ($key in @('x', 'y', 'width', 'height')) {
                $number = 0.0
                if (-not [double]::TryParse([string]$buttonRect.$key,
                    [System.Globalization.NumberStyles]::Float,
                    [System.Globalization.CultureInfo]::InvariantCulture, [ref]$number) -or
                    [double]::IsNaN($number) -or [double]::IsInfinity($number)) { throw 'invalid_button_rect' }
            }
        }
        if ($null -eq $buttonRect -or $buttonRect.width -le 0 -or $buttonRect.height -le 0 -or
            $buttonRect.x -lt 0 -or $buttonRect.y -lt 0 -or
            $buttonRect.x + $buttonRect.width -gt $capture.Width -or
            $buttonRect.y + $buttonRect.height -gt $capture.Height) { throw 'invalid_button_rect' }

        # Require exactly one whole OCR phrase, on one text line, at the captured
        # location. Coordinates alone never authorize a click.
        $parts = @($label -split ' ')
        $matches = @()
        foreach ($line in ($words | Group-Object line)) {
            $lineWords = @($line.Group | Sort-Object x)
            for ($i = 0; $i -le $lineWords.Count - $parts.Count; $i++) {
                $candidate = @($lineWords[$i..($i + $parts.Count - 1)])
                $candidateText = (($candidate | ForEach-Object { $_.text }) -join ' ').Trim().ToLowerInvariant()
                if ($candidateText -cne $label) { continue }
                $left = ($candidate | Measure-Object x -Minimum).Minimum
                $top = ($candidate | Measure-Object y -Minimum).Minimum
                $right = ($candidate | ForEach-Object { $_.x + $_.width } | Measure-Object -Maximum).Maximum
                $bottom = ($candidate | ForEach-Object { $_.y + $_.height } | Measure-Object -Maximum).Maximum
                $matches += @{ x = $left; y = $top; width = $right - $left; height = $bottom - $top }
            }
        }
        if ($matches.Count -ne 1) { throw 'button_not_unique' }
        $found = $matches[0]
        foreach ($key in @('x', 'y', 'width', 'height')) {
            if ([math]::Abs([double]$found[$key] - [double]$buttonRect.$key) -gt 3) { throw 'button_moved' }
        }
        $deliveredTick = [SwchEpicVision.Native]::Click($handle, $processId, $capture.X, $capture.Y,
            $capture.Width, $capture.Height, $expectedTick, $capture.Hash,
            [int][math]::Round($found.x + $found.width / 2),
            [int][math]::Round($found.y + $found.height / 2))
        Write-Result @{ ok = $true; version = 1; clicked = $true; button = $label; last_input_tick = $deliveredTick }
    } finally { $capture.Dispose() }
} catch {
    # WinRT/managed exceptions can contain file paths or page content. Emit only
    # our fixed public error identifiers, never exception stacks or screenshots.
    $failure = $_.Exception
    while ($null -ne $failure.InnerException) { $failure = $failure.InnerException }
    $code = [string]$failure.Message
    if ($code -notmatch '^[a-z][a-z0-9_]{2,80}$') { $code = 'vision_unavailable' }
    Write-Result @{ ok = $false; version = 1; error = $code }
    exit 1
}
