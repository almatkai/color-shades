#!/usr/bin/env python3
"""Verify a package ZIP offline and drive its extracted executable. Never installs.

Generic checks: safe unique paths, exact inventory and SHA256s, sidecars when
present, --protocol-info identity, hello, initialize, then for every command on
this platform: session.open (required text arguments use their manifest
placeholder), invoke the first `copy` action when one exists, dispose. The fake
host answers only reverse methods the manifest grants, records them, and performs
no real clipboard/shell/GUI effect.
"""
import argparse, hashlib, json, os, queue, stat, subprocess, tempfile, threading, uuid, zipfile
from pathlib import Path, PurePosixPath

LIMIT = 16 * 1024 * 1024


def granted(method, capabilities):
    capability = method.removeprefix('host.')
    return method in capabilities or capability in capabilities or capability.split('.')[0] in capabilities


def arguments(command):
    values = {}
    for argument in command.get('arguments', []):
        if argument.get('required'):
            assert argument.get('type', 'text') == 'text' and argument.get('placeholder'), 'Smoke needs a text placeholder for ' + argument['name']
            values[argument['name']] = argument['placeholder']
    return values


def verify_sidecars(path, manifest, archive):
    sidecar = path.with_name(path.name + '.sha256')
    if sidecar.exists():
        assert sidecar.read_text().split()[0] == hashlib.sha256(path.read_bytes()).hexdigest(), 'Archive sidecar mismatch'
    stem = path.name.removesuffix('.zip')
    artifact = path.with_name(stem + '.artifact.json')
    if artifact.exists():
        info = json.loads(artifact.read_text())
        assert info['sha256'] == hashlib.sha256(path.read_bytes()).hexdigest(), 'Artifact digest mismatch'
        assert manifest['files'][info['executable']] == info['executableSha256'], 'Executable digest mismatch'
        assert hashlib.sha256(archive.read(info['executable'])).hexdigest() == info['executableSha256']
        exe_sidecar = path.with_name(stem + '.executable.sha256')
        if exe_sidecar.exists():
            assert exe_sidecar.read_text().split()[0] == info['executableSha256'], 'Executable sidecar mismatch'


def smoke(path, keep=None):
    with tempfile.TemporaryDirectory() as temp, zipfile.ZipFile(path) as archive:
        root = Path(temp) / 'package'
        root.mkdir()
        names = archive.namelist()
        assert len(names) == len(set(names)), 'Duplicate ZIP paths'
        assert len({n.lower() for n in names}) == len(names), 'Case-folded duplicate ZIP paths'
        for item in archive.infolist():
            name = PurePosixPath(item.filename)
            assert not name.is_absolute() and '..' not in name.parts and '\\' not in item.filename and ':' not in item.filename, 'Unsafe ZIP path'
            assert not stat.S_ISLNK(item.external_attr >> 16), 'Symlink ZIP entry'
        manifest = json.loads(archive.read('extension.json'))
        inventory = manifest['files']
        assert set(names) == set(inventory) | {'extension.json'}, 'ZIP/inventory mismatch'
        for name, digest in inventory.items():
            assert hashlib.sha256(archive.read(name)).hexdigest() == digest, 'SHA inventory mismatch: ' + name
        for required in ['LICENSE', 'NOTICE']:
            assert required in inventory, 'Missing ' + required
        if manifest.get('icon') and not manifest['icon'].startswith('https://'):
            assert manifest['icon'] in inventory, 'Icon not packaged'
        verify_sidecars(path, manifest, archive)
        archive.extractall(root)
        windows = os.name == 'nt'
        platform = 'Windows' if windows else 'macOS'
        slug = manifest['slug']
        binary = root / 'bin' / ('tezbar-' + slug + ('.exe' if windows else ''))
        if not windows:
            assert os.access(binary, os.X_OK) or (archive.getinfo('bin/tezbar-' + slug).external_attr >> 16) & 0o111, 'Executable bit missing'
            binary.chmod(0o755)
        env = {k: v for k, v in os.environ.items() if k in ('SystemRoot', 'WINDIR', 'TEMP', 'TMP')}
        env['PATH'] = os.path.join(env.get('SystemRoot', 'C:/Windows'), 'System32') if windows else '/usr/bin:/bin'
        info = json.loads(subprocess.check_output([str(binary), '--protocol-info'], cwd=root, env=env, timeout=10))
        assert info['id'] == manifest['id'] and info['version'] == manifest['version'] and info['protocol'] == 1
        commands = [c for c in manifest['commands'] if c['runtime'] == 'rust' and platform in c['platforms']]
        assert set(info['commands']) == {c['name'] for c in commands}, 'Binary/manifest command mismatch'
        process = subprocess.Popen([str(binary)], cwd=root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=None)
        messages = queue.Queue()
        recorded = []

        def reader():
            try:
                while True:
                    line = process.stdout.readline(LIMIT + 1)
                    if not line:
                        messages.put(EOFError('Unexpected EOF'))
                        return
                    if len(line) > LIMIT:
                        raise ValueError('Oversized frame')
                    messages.put(json.loads(line))
            except Exception as error:
                messages.put(error)

        threading.Thread(target=reader, daemon=True).start()

        def receive():
            value = messages.get(timeout=10)
            if isinstance(value, Exception):
                raise value
            return value

        def send(value):
            process.stdin.write((json.dumps(value) + '\n').encode())
            process.stdin.flush()

        def request(method, params):
            ident = 'h:' + str(uuid.uuid4())
            send({'type': 'request', 'id': ident, 'method': method, 'params': params})
            while True:
                frame = receive()
                if frame['type'] == 'response' and frame['id'] == ident:
                    assert 'error' not in frame, frame
                    return frame['result']
                if frame['type'] == 'request':
                    assert granted(frame['method'], manifest['capabilities']), 'Ungranted reverse request: ' + frame['method']
                    recorded.append((frame['method'], frame['params']))
                    send({'type': 'response', 'id': frame['id'], 'result': {'ok': True}})
                    continue
                assert frame['type'] in ('event', 'cancel'), 'Unexpected frame: ' + str(frame)

        try:
            hello = receive()
            assert hello['type'] == 'hello' and hello['id'] == manifest['id'] and hello['version'] == manifest['version'] and hello['protocol'] == 1
            data = Path(temp) / 'data'
            ready = request('initialize', {'protocol': 1, 'extensionId': manifest['id'], 'platform': platform, 'packageDir': str(root), 'dataDir': str(data), 'tempDir': str(Path(temp) / 'temp'), 'grantedCapabilities': manifest['capabilities'], 'reason': 'command', 'migrationPaths': {}})
            assert ready['ready'] is True
            report = []
            for index, command in enumerate(commands):
                session = f'zip-smoke-{index}'
                view = request('session.open', {'sessionId': session, 'commandName': command['name'], 'preferences': {}, 'argumentValues': arguments(command)})
                assert view['mode'] == command.get('mode', 'view'), view
                entry = {'command': command['name'], 'mode': view['mode'], 'root': view.get('root', {}).get('type')}
                copy = next((a for a in view.get('actions', []) if a.get('kind') == 'copy'), None)
                if copy:
                    before = len(recorded)
                    result = request('session.call', {'sessionId': session, 'operation': 'extension:invoke-action', 'payload': {'actionId': copy['id']}})
                    assert result['mode'] in ('unchanged', 'view', 'no-view'), result
                    assert len(recorded) == before + 1, 'Copy action issued no recorded host request'
                    entry['copy'] = {'action': copy['title'], 'method': recorded[-1][0], 'text': recorded[-1][1].get('text')}
                request('session.dispose', {'sessionId': session})
                report.append(entry)
            request('shutdown', {})
            assert process.wait(timeout=5) == 0, 'Process did not exit cleanly'
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            process.stdin.close()
            process.stdout.close()
        if keep:
            keep.write_text(json.dumps({'archive': path.name, 'sessions': report}, indent=2) + '\n')
        print(slug, 'ZIP inventory/sidecars + protocol-info/hello/initialize/open/action/dispose/shutdown passed (offline, recording host, no installation)')
        print(json.dumps(report))


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('archive', type=Path)
    p.add_argument('--report', type=Path)
    a = p.parse_args()
    smoke(a.archive, a.report)
