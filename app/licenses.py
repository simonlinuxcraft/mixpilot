#!/usr/bin/env python3
"""Writes ui/third-party-licenses.txt: every crate compiled into the app and the core, with its license texts.
Run after dependency changes: python3 licenses.py"""
import hashlib, json, pathlib, subprocess

HERE = pathlib.Path(__file__).resolve().parent
NAMES = ('license', 'licence', 'copying', 'notice', 'unlicense')


def runtime_packages(manifest):
    meta = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--format-version', '1', '--locked', '--filter-platform', 'x86_64-unknown-linux-gnu',
         '--manifest-path', str(manifest)]))
    pkgs = {p['id']: p for p in meta['packages']}
    nodes = {n['id']: n for n in meta['resolve']['nodes']}
    seen, todo = set(), [meta['resolve']['root']]
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]['deps']:
            # normal dependencies only: build scripts and tests do not end up in the binary
            if any(k['kind'] is None for k in d['dep_kinds']):
                todo.append(d['pkg'])
    return [pkgs[i] for i in seen if pkgs[i]['source']]


def texts(pkg):
    root = pathlib.Path(pkg['manifest_path']).parent
    files = sorted(f for f in root.iterdir() if f.is_file() and f.name.lower().startswith(NAMES))
    if pkg.get('license_file'):
        files.append(root / pkg['license_file'])
    out = []
    for f in dict.fromkeys(files):
        try:
            out.append(f.read_text(errors='replace').strip())
        except OSError:
            pass
    return out


crates = {}
for m in (HERE / 'Cargo.toml', HERE.parent / 'core' / 'Cargo.toml'):
    for p in runtime_packages(m):
        crates[(p['name'], p['version'])] = p

groups = {}  # license text -> crates using it
no_text = []
for (name, ver), p in sorted(crates.items()):
    found = texts(p)
    if not found:
        no_text.append(f"{name} {ver} ({p.get('license') or 'unknown'})")
    for t in found:
        groups.setdefault(t, []).append(f'{name} {ver}')

out = ['Mixpilot uses the following open source software.', '',
       'Font: Barlow Semi Condensed, SIL Open Font License 1.1 (fonts/OFL.txt).', '']
for t, users in sorted(groups.items(), key=lambda g: g[1][0]):
    out += ['=' * 72, 'Used by: ' + ', '.join(users), '-' * 72, t, '']
if no_text:
    out += ['=' * 72, 'Crates without a bundled license file (license as declared):', *no_text, '']
(HERE / 'ui' / 'third-party-licenses.txt').write_text('\n'.join(out))
print(f'{len(crates)} crates, {len(groups)} distinct license texts, {len(no_text)} without file')
