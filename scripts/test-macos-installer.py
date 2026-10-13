#!/usr/bin/env python3
"""Install and reinstall only on an isolated macOS CI runner."""
import argparse
import hashlib
import os
import pathlib
import plistlib
import struct
import subprocess
import sys


def run(*arguments, **kwargs):
    return subprocess.run([str(arg) for arg in arguments], check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installer", required=True, type=pathlib.Path)
    parser.add_argument("--bundle", required=True, type=pathlib.Path)
    args = parser.parse_args()
    if sys.platform != "darwin" or os.environ.get("CI") != "true" or not os.environ.get("RUNNER_TEMP"):
        parser.error("Run installation tests on an isolated macOS GitHub Actions runner.")
    installer = args.installer.resolve(strict=True)
    source = args.bundle.resolve(strict=True)
    app = pathlib.Path("/Applications/SWCH.app")
    if app.exists():
        raise RuntimeError("Refusing to overwrite an existing application before the test")
    run("sudo", "/usr/sbin/installer", "-pkg", installer, "-target", "/")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    assert info == plistlib.loads((source / "Contents/Info.plist").read_bytes())
    assert info["CFBundlePackageType"] == "APPL" and info["CFBundleDisplayName"] == "SWCH"
    assert info["CFBundleName"] == "SWCH" and info["CFBundleIdentifier"] == "com.kabarab.swch"
    assert info["LSMinimumSystemVersion"] == "13.5"
    for file in source.rglob("*"):
        if file.is_file():
            installed = app / file.relative_to(source)
            assert installed.is_file(), installed
            assert hashlib.sha256(file.read_bytes()).digest() == hashlib.sha256(installed.read_bytes()).digest(), installed
    for file in ["Contents/MacOS/swch", "Contents/MacOS/swch-server", "Contents/Resources/runtime/node"]:
        assert os.access(app / file, os.X_OK)
        architecture = run("/usr/bin/lipo", "-archs", app / file, capture_output=True, text=True).stdout
        assert "arm64" in architecture
        binary = (app / file).read_bytes()
        assert binary[:4] == b'\xcf\xfa\xed\xfe'
        position = 32
        minimum = None
        for _ in range(struct.unpack_from('<I', binary, 16)[0]):
            command, length = struct.unpack_from('<II', binary, position)
            if command == 0x32:
                minimum = struct.unpack_from('<I', binary, position + 12)[0]
            position += length
        assert minimum is not None and minimum <= (13 << 16 | 5 << 8), (file, minimum)
    run("/usr/bin/codesign", "--verify", "--deep", "--strict", app)
    register = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
    run(register, "-f", app)
    registry = run(register, "-dump", capture_output=True, text=True).stdout
    assert str(app) in registry and "com.kabarab.swch" in registry
    data = pathlib.Path.home() / "Library/Application Support/swch"
    data.mkdir(parents=True, exist_ok=True)
    sentinel = data / "installer-ci-preservation-test.txt"
    assert not sentinel.exists()
    sentinel.write_text("preserve data", encoding="utf-8")
    run("sudo", "/usr/sbin/installer", "-pkg", installer, "-target", "/")
    assert sentinel.read_text(encoding="utf-8") == "preserve data"
    sentinel.unlink()
    print("macOS installation passed: application identity, payload hashes, ARM64, LaunchServices, reinstall and data preservation.")


if __name__ == "__main__":
    main()
