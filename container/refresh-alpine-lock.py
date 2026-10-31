#!/usr/bin/env python3
"""Re-pin the Alpine packages of tools/build/inputs.lock.json that Alpine's mirror no longer serves.

Alpine keeps only the newest revision of a package in a release branch, so a locked revision that
received an update disappears (HTTP 404) and tools/build/prepare.py cannot fetch it. This lets
apk resolve the same package set again (current revisions, with any new dependency) in a scratch
copy of the locked minirootfs, then pins every package that changed: file name, URL and SHA-256 of
the downloaded archive, and the installed inventory prepare.py compares. Unchanged entries keep
their place and hashes. Nothing happens while every locked package is cached or still served.

Run from a Y2Linux checkout (`just refresh-alpine-lock` and container/image.sh do):

    python3 refresh-alpine-lock.py           # refresh when needed
    python3 refresh-alpine-lock.py --check   # only report (exit 3 when stale)

Needs network and bubblewrap (the build container has both). The changed lock is a source change:
review and commit it before tools/production/build.py, which refuses an uncommitted tree.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

PROJECT = Path.cwd().resolve()
if not (PROJECT / 'tools/build/prepare.py').is_file():
    raise SystemExit(f'refresh-alpine-lock: {PROJECT} is not a Y2Linux checkout')
sys.path.insert(0, str(PROJECT))
from tools.build.prepare import extract, installed, sha  # noqa: E402

LOCK = PROJECT / 'tools/build/inputs.lock.json'
CACHE = PROJECT / '.cache'
BRANCH = 'https://dl-cdn.alpinelinux.org/alpine/v3.24'
REPOSITORIES = ('main', 'community')
# --check's exit code for a stale lock (1 is any other failure, such as no network).
STALE = 3


def served(url):
    request = urllib.request.Request(url, method='HEAD')
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status == 200
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return False
        raise


def download(url, target):
    partial = target.with_suffix(target.suffix + '.partial')
    with urllib.request.urlopen(url, timeout=120) as source, partial.open('wb') as destination:
        shutil.copyfileobj(source, destination)
    partial.replace(target)


def stale(lock):
    """Locked archives neither cached nor served any more."""
    return [item for item in lock['apk_packages']
            if not (CACHE / 'apks' / item['name']).exists() and not served(item['url'])]


def package_names(lock):
    """The package each locked archive installs: `name-version.apk` against the inventory."""
    versions = dict(entry.split('=', 1) for entry in lock['installed_packages'])
    names = {}
    for item in lock['apk_packages']:
        name = next((package for package, version in versions.items()
                     if item['name'] == f'{package}-{version}.apk'), None)
        if name is None:
            raise SystemExit('no inventory entry for ' + item['name'])
        names[item['name']] = name
    return names


def resolve(lock, names):
    """The inventory apk reaches today from the locked minirootfs, and the minirootfs's own."""
    base = next(item for item in lock['base_archives'] if item['destination'] == 'rootfs')
    archive = CACHE / 'downloads' / base['name']
    if not archive.exists():
        archive.parent.mkdir(parents=True, exist_ok=True)
        download(base['url'], archive)
    if sha(archive) != base['sha256']:
        raise SystemExit('minirootfs checksum mismatch: ' + base['name'])
    with tempfile.TemporaryDirectory(dir=CACHE) as scratch:
        root = Path(scratch) / 'root'
        root.mkdir()
        extract(archive, root)
        minirootfs = set(installed(root))
        (root / 'etc/apk/repositories').write_text(
            ''.join(f'{BRANCH}/{repository}\n' for repository in REPOSITORIES))
        subprocess.run([
            'bwrap', '--unshare-all', '--share-net', '--uid', '0', '--gid', '0', '--die-with-parent',
            '--bind', str(root), '/', '--proc', '/proc', '--dev', '/dev',
            '--ro-bind', '/etc/resolv.conf', '/etc/resolv.conf', '--',
            '/sbin/apk', 'add', '--upgrade', '--no-cache', '--quiet', *sorted(set(names.values())),
        ], check=True)
        return installed(root), minirootfs


def archive_url(file_name):
    for repository in REPOSITORIES:
        url = f'{BRANCH}/{repository}/x86_64/{file_name}'
        if served(url):
            return url
    raise SystemExit('not served by ' + BRANCH + ': ' + file_name)


def refresh(lock):
    names = package_names(lock)
    inventory, minirootfs = resolve(lock, names)
    wanted = {entry.split('=', 1)[0]: entry.split('=', 1)[1]
              for entry in inventory if entry not in minirootfs}
    pinned = {names[item['name']]: item for item in lock['apk_packages']}
    (CACHE / 'apks').mkdir(parents=True, exist_ok=True)
    changes = []

    def entry(package):
        file_name = f'{package}-{wanted[package]}.apk'
        old = pinned.get(package)
        if old and old['name'] == file_name:
            return old
        url = archive_url(file_name)
        target = CACHE / 'apks' / file_name
        download(url, target)
        changes.append((old['name'] if old else '(new)', file_name))
        return {'name': file_name, 'url': url, 'sha256': sha(target)}

    packages = [entry(names[item['name']]) for item in lock['apk_packages']
                if names[item['name']] in wanted]
    packages += [entry(package) for package in sorted(wanted.keys() - pinned.keys())]
    dropped = sorted(pinned.keys() - wanted.keys())
    lock['apk_packages'] = packages
    lock['installed_packages'] = inventory
    return changes, dropped


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--check', action='store_true', help='only report stale locked packages')
    args = parser.parse_args()
    lock = json.loads(LOCK.read_text())
    missing = stale(lock)
    if not missing:
        print('Alpine lock: every locked package is cached or served.')
        return 0
    print('Alpine lock: no longer served: ' + ', '.join(item['name'] for item in missing))
    if args.check:
        return STALE
    changes, dropped = refresh(lock)
    LOCK.write_text(json.dumps(lock, indent=2) + '\n')
    # prepare.py refuses an environment built from another lock; it is a cache, rebuilt from the lock.
    marker = CACHE / 'environment/.y2-build-lock'
    if (CACHE / 'environment').exists() and (not marker.exists()
                                              or marker.read_text().strip() != sha(LOCK)):
        shutil.rmtree(CACHE / 'environment')
    for old, new in changes:
        print(f'  {old} -> {new}')
    for package in dropped:
        print(f'  {package}: no longer needed')
    print(f'Alpine lock refreshed ({len(changes)} archives); review and commit {LOCK.relative_to(PROJECT)}.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
