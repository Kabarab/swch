"""Keep native application metadata aligned with Cargo.toml before packaging."""
import argparse
from pathlib import Path
import plistlib
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        parser.error('Native packages require a stable numeric version')
    resource = ROOT / 'src/windows_app.rc'
    content = resource.read_text(encoding='utf-8')
    updated, numeric = re.subn(r'(?m)^(FILEVERSION|PRODUCTVERSION) \d+,\d+,\d+,\d+',
        lambda match: match[1] + ' ' + version.replace('.', ',') + ',0', content)
    updated, strings = re.subn(r'(VALUE "(?:FileVersion|ProductVersion)", ")[^"\\]+',
        lambda match: match[1] + version, updated)
    if numeric != 2 or strings != 2:
        parser.error('Expected both numeric and string Windows version fields')
    plist = ROOT / 'swch.app/Contents/Info.plist'
    info = plistlib.loads(plist.read_bytes())
    stale = updated != content or any(info.get(key) != version for key in
        ('CFBundleVersion', 'CFBundleShortVersionString'))
    if args.check:
        if stale:
            parser.error('Native version metadata differs from Cargo.toml; run this script without --check')
    else:
        resource.write_text(updated, encoding='utf-8', newline='\n')
        info.update(CFBundleVersion=version, CFBundleShortVersionString=version)
        plist.write_bytes(plistlib.dumps(info, sort_keys=False))
    print(f'Native metadata: {version}')


if __name__ == '__main__':
    main()
