#!/usr/bin/env python3
"""Build the native macOS installer for /Applications/SWCH.app."""
import argparse
import pathlib
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE_ID = "com.kabarab.swch.installer"


def run(*arguments):
    subprocess.run([str(arg) for arg in arguments], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=pathlib.Path, default=ROOT / "swch.app")
    parser.add_argument("--version")
    parser.add_argument("--output-dir", type=pathlib.Path, default=ROOT / "output/installers")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("Run this builder on macOS with Apple's pkgbuild and productbuild.")
    version = args.version or re.search(r'^version\s*=\s*"([^\"]+)"',
        (ROOT / "Cargo.toml").read_text(), re.M)[1]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        parser.error("A stable numeric release version is required.")
    bundle = args.bundle.resolve(strict=True)
    info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    if (info.get("CFBundleIdentifier") != "com.kabarab.swch"
            or info.get("CFBundlePackageType") != "APPL"
            or info.get("CFBundleShortVersionString") != version
            or info.get("CFBundleVersion") != version):
        parser.error("Application identity or bundle version does not match the release.")
    for file in ["Contents/MacOS/swch", "Contents/MacOS/swch-server",
                 "Contents/Resources/runtime/node", "Contents/Resources/iconfile.icns",
                 "Contents/Resources/web/index.html"]:
        if not (bundle / file).is_file():
            parser.error("Missing bundle file: " + file)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    output = args.output_dir.resolve() / f"SWCH-Setup-{version}-arm64.pkg"
    with tempfile.TemporaryDirectory(prefix="swch-installer-") as directory:
        work = pathlib.Path(directory)
        payload = work / "payload"
        installed_bundle = payload / "Applications/SWCH.app"
        installed_bundle.parent.mkdir(parents=True)
        run("/usr/bin/ditto", bundle, installed_bundle)
        components = work / "components.plist"
        run("/usr/bin/pkgbuild", "--analyze", "--root", payload, components)
        records = plistlib.loads(components.read_bytes())
        if len(records) != 1 or records[0]["RootRelativeBundlePath"] != "Applications/SWCH.app":
            raise RuntimeError("Unexpected application component layout")
        records[0].update(BundleIsRelocatable=False, BundleIsVersionChecked=True,
                          BundleHasStrictIdentifier=True, BundleOverwriteAction="upgrade")
        components.write_bytes(plistlib.dumps(records))
        scripts = work / "scripts"
        scripts.mkdir()
        postinstall = scripts / "postinstall"
        shutil.copyfile(ROOT / "packaging/macos/postinstall", postinstall)
        postinstall.chmod(0o755)
        component = work / "swch-component.pkg"
        run("/usr/bin/pkgbuild", "--root", payload, "--component-plist", components,
            "--install-location", "/", "--identifier", PACKAGE_ID,
            "--version", version, "--scripts", scripts, component)
        distribution = ET.Element("installer-gui-script", minSpecVersion="2")
        ET.SubElement(distribution, "title").text = "SWCH"
        ET.SubElement(distribution, "options", customize="never", hostArchitectures="arm64")
        ET.SubElement(distribution, "domains", enable_anywhere="false",
                      enable_currentUserHome="false", enable_localSystem="true")
        systems = ET.SubElement(distribution, "allowed-os-versions")
        ET.SubElement(systems, "os-version", min=info["LSMinimumSystemVersion"])
        choices = ET.SubElement(distribution, "choices-outline")
        ET.SubElement(choices, "line", choice="swch")
        choice = ET.SubElement(distribution, "choice", id="swch", visible="false")
        ET.SubElement(choice, "pkg-ref", id=PACKAGE_ID)
        ET.SubElement(distribution, "pkg-ref", id=PACKAGE_ID,
                      version=version).text = component.name
        definition = work / "Distribution.xml"
        ET.ElementTree(distribution).write(definition, encoding="utf-8", xml_declaration=True)
        run("/usr/bin/productbuild", "--distribution", definition,
            "--package-path", work, output)
    print(f"Built {output}: installs SWCH.app into /Applications")


if __name__ == "__main__":
    main()
