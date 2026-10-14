[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Add-Type -AssemblyName System.Drawing
if (-not ('SwchIconPacker' -as [type])) {
    $drawingAssemblies = @([Drawing.Bitmap].Assembly.Location, [Drawing.Rectangle].Assembly.Location, 'System.Runtime') | Select-Object -Unique
    $drawingAssemblies += @(Get-ChildItem -LiteralPath $PSHOME -Filter 'System.Private.Windows.*.dll' | ForEach-Object FullName)
    Add-Type -ReferencedAssemblies $drawingAssemblies -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;

public static class SwchIconPacker {
    static byte[] Frame(string source, int size) {
        using (var image = new Bitmap(source))
        using (var bitmap = new Bitmap(size, size, PixelFormat.Format32bppPArgb))
        using (var graphics = Graphics.FromImage(bitmap))
        using (var attributes = new ImageAttributes())
        using (var stream = new MemoryStream()) {
            if (image.Width != image.Height || image.GetPixel(0, 0).A != 0)
                throw new InvalidDataException("Icon master must be square with transparent margins.");
            graphics.CompositingMode = CompositingMode.SourceCopy;
            graphics.CompositingQuality = CompositingQuality.HighQuality;
            graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            attributes.SetWrapMode(WrapMode.TileFlipXY);
            graphics.DrawImage(image, new Rectangle(0, 0, size, size), 0, 0,
                image.Width, image.Height, GraphicsUnit.Pixel, attributes);
            bitmap.Save(stream, ImageFormat.Png);
            return stream.ToArray();
        }
    }

    public static void Ico(string source, string destination) {
        int[] sizes = {16, 20, 24, 32, 40, 48, 64, 128, 256};
        var frames = new byte[sizes.Length][];
        for (int i = 0; i < sizes.Length; i++) frames[i] = Frame(source, sizes[i]);
        using (var stream = File.Create(destination))
        using (var writer = new BinaryWriter(stream)) {
            writer.Write((ushort)0); writer.Write((ushort)1); writer.Write((ushort)sizes.Length);
            uint offset = (uint)(6 + 16 * sizes.Length);
            for (int i = 0; i < sizes.Length; i++) {
                writer.Write((byte)(sizes[i] % 256)); writer.Write((byte)(sizes[i] % 256));
                writer.Write((byte)0); writer.Write((byte)0);
                writer.Write((ushort)1); writer.Write((ushort)32);
                writer.Write((uint)frames[i].Length); writer.Write(offset);
                offset += (uint)frames[i].Length;
            }
            foreach (var frame in frames) writer.Write(frame);
        }
    }

    static void BigEndian(BinaryWriter writer, uint number) {
        writer.Write(new byte[] {(byte)(number >> 24), (byte)(number >> 16),
            (byte)(number >> 8), (byte)number});
    }

    public static void Icns(string source, string destination) {
        string[] types = {"icp4", "icp5", "icp6", "ic07", "ic08", "ic09", "ic10", "ic11", "ic12", "ic13", "ic14"};
        int[] sizes = {16, 32, 64, 128, 256, 512, 1024, 32, 64, 256, 512};
        using (var stream = File.Create(destination))
        using (var writer = new BinaryWriter(stream)) {
            writer.Write(System.Text.Encoding.ASCII.GetBytes("icns")); BigEndian(writer, 0);
            for (int i = 0; i < sizes.Length; i++) {
                byte[] frame = Frame(source, sizes[i]);
                writer.Write(System.Text.Encoding.ASCII.GetBytes(types[i]));
                BigEndian(writer, (uint)(8 + frame.Length)); writer.Write(frame);
            }
            uint length = (uint)stream.Length;
            stream.Position = 4; BigEndian(writer, length);
        }
    }
}
'@
}
$master = Join-Path $projectRoot 'src/assets/swch-logo.png'
[SwchIconPacker]::Ico($master, (Join-Path $projectRoot 'src/assets/swch.ico'))
[SwchIconPacker]::Icns($master, (Join-Path $projectRoot 'swch.app/Contents/Resources/iconfile.icns'))
Write-Host 'Generated Windows ICO (16-256px) and macOS ICNS (16-1024px, including Retina).'
