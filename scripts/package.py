#!/usr/bin/env python3
"""Build a schema-3 root-layout ZIP, its SHA256 inventory and sidecars. No uploads.

Generic: identity, commands and platforms come from extension.json; the binary's
--protocol-info must agree. Layout: extension.json, bin/tezbar-<slug>[.exe],
resources/**, licenses/**, LICENSE, NOTICE.
"""
import argparse, hashlib, json, re, shutil, stat, struct, subprocess, sys, tempfile, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {'aarch64-apple-darwin': False, 'x86_64-pc-windows-msvc': True}
SDK_REV = 'aaec571673ab64a85a136cf6bd054ef660c4d93c'
# Same MIT terms as the repository's root LICENSE, stored with real line breaks.
LICENSE_SOURCE = ROOT / 'licenses' / 'color-shades-port-MIT.txt'
DATE = (2026, 1, 1, 0, 0, 0)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check_sdk():
    text = (ROOT / 'Cargo.toml').read_text()
    match = re.search(r'^tezbar-extension-sdk\s*=\s*\{([^}\n]*)\}', text, re.M)
    fields = match.group(1) if match else ''
    revision = re.search(r'\brev\s*=\s*"([0-9a-f]{40})"', fields)
    if not revision or revision.group(1) != SDK_REV or re.search(r'\b(path|branch|tag)\s*=', fields) or not re.search(r'\bgit\s*=\s*"https://github.com/almatkai/tezbar-extension-sdk-rust.git"', fields):
        raise SystemExit('Release blocked: the published SDK must use approved immutable revision ' + SDK_REV)
    lock = (ROOT / 'Cargo.lock').read_text()
    entry = re.search(r'\[\[package\]\]\nname = "tezbar-extension-sdk"(.*?)(?=\[\[package\]\]|\Z)', lock, re.S)
    if not entry or not re.search(r'source = "git\+[^"\n]+#' + revision.group(1) + r'"', entry.group(1)):
        raise SystemExit('Release blocked: Cargo.lock must contain the same immutable SDK git revision.')


def validate_manifest(manifest):
    assert manifest['schemaVersion'] == 3 and manifest['runtime'] == 'rust'
    assert manifest['execution'] == 'process' and manifest['protocolVersion'] == 1
    assert manifest['id'] == 'com.tezbar.native.' + manifest['slug'], 'Canonical id mismatch'
    for key in ['name', 'version', 'repository', 'commands', 'preferences', 'replaces', 'capabilities', 'artifacts']:
        assert key in manifest, 'Missing manifest field ' + key
    assert manifest['replaces'], 'replaces must list the original package'
    names = [c['name'] for c in manifest['commands']]
    assert len(names) == len(set(names)), 'Duplicate command'
    icon = manifest.get('icon')
    if icon and not icon.startswith('https://'):
        assert (ROOT / icon).is_file(), 'Manifest icon is not packaged: ' + icon


def check_target(target, binary):
    with binary.open('rb') as file:
        header = file.read(64)
        if TARGETS[target]:
            assert header[:2] == b'MZ', 'Expected a Windows PE executable'
            file.seek(struct.unpack_from('<I', header, 60)[0])
            pe = file.read(6)
            assert pe[:4] == b'PE\x00\x00' and struct.unpack_from('<H', pe, 4)[0] == 0x8664, 'Expected Windows x64 machine type'
        else:
            assert header[:4] == b'\xcf\xfa\xed\xfe' and struct.unpack_from('<I', header, 4)[0] == 0x0100000c, 'Expected arm64 Mach-O executable'


def build(target, binary, output):
    check_sdk()
    manifest = json.loads((ROOT / 'extension.json').read_text())
    validate_manifest(manifest)
    slug = manifest['slug']
    windows = TARGETS[target]
    executable = f'bin/tezbar-{slug}' + ('.exe' if windows else '')
    platform = 'Windows' if windows else 'macOS'
    expected = [c['name'] for c in manifest['commands'] if c['runtime'] == 'rust' and platform in c['platforms']]
    if sys.platform != ('win32' if windows else 'darwin'):
        raise SystemExit('Package each target on its own OS so the binary identity is executed and verified.')
    check_target(target, binary)
    info = json.loads(subprocess.check_output([str(binary.resolve()), '--protocol-info'], timeout=10))
    assert info['id'] == manifest['id'] and info['version'] == manifest['version'] and info['protocol'] == 1, 'Binary identity mismatch'
    assert set(info['commands']) == set(expected), 'Binary/manifest platform coverage mismatch'
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        stage = Path(temp)
        (stage / 'bin').mkdir()
        shutil.copy2(binary, stage / executable)
        for name in ['resources', 'licenses']:
            shutil.copytree(ROOT / name, stage / name)
        shutil.copy2(LICENSE_SOURCE, stage / 'LICENSE')
        shutil.copy2(ROOT / 'NOTICE', stage / 'NOTICE')
        files = {}
        for path in sorted(stage.rglob('*')):
            if path.is_symlink():
                raise ValueError('Symlinks cannot be packaged')
            if path.is_file():
                files[path.relative_to(stage).as_posix()] = digest(path)
        manifest['files'] = files
        # The parent supplies real published URLs/digests in the trusted host catalog.
        manifest['artifacts'] = {}
        (stage / 'extension.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
        archive = output / f'{slug}-{target}.zip'
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as z:
            for path in sorted(stage.rglob('*')):
                if path.is_file():
                    name = path.relative_to(stage).as_posix()
                    entry = zipfile.ZipInfo(name, date_time=DATE)
                    entry.create_system = 3
                    mode = 0o755 if name == executable else 0o644
                    entry.external_attr = (stat.S_IFREG | mode) << 16
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    z.writestr(entry, path.read_bytes())
        with zipfile.ZipFile(archive) as z:
            packaged = json.loads(z.read('extension.json'))
            assert set(z.namelist()) == set(files) | {'extension.json'}
            for name, sha in packaged['files'].items():
                assert hashlib.sha256(z.read(name)).hexdigest() == sha
        artifact = {'target': target, 'sha256': digest(archive), 'executable': executable, 'executableSha256': files[executable]}
        (output / f'{slug}-{target}.artifact.json').write_text(json.dumps(artifact, indent=2) + '\n')
        (output / f'{archive.name}.sha256').write_text(f"{artifact['sha256']}  {archive.name}\n")
        (output / f'{slug}-{target}.executable.sha256').write_text(f"{artifact['executableSha256']}  {executable}\n")
        (output / f'{slug}-{target}.files.json').write_text(json.dumps(files, indent=2, sort_keys=True) + '\n')
        print(archive)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--target', required=True, choices=TARGETS)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    build(args.target, args.binary, args.output)
