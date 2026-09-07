#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
# rue backstop artifact: plan open_mgmt_port on fw-win-02 (os windows), instance golden, language python. Rendered by rue-render; do not edit.
import hashlib
import os
import shutil
import subprocess
import sys
import time

ROOT = 'C:\\ProgramData\\rue'
INSTANCE = 'golden'
INST = os.path.join(ROOT, 'instances', INSTANCE)
RUN = ['powershell.exe', '-NoProfile', '-NonInteractive', '-Command']


def p(*parts):
    return os.path.join(INST, *parts)


def read(path):
    with open(path, encoding='utf-8') as f:
        return f.read()


def sha(path):
    if not os.path.isfile(path):
        return 'missing'
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


def recorded(marker, path):
    for line in read(marker).splitlines():
        f = line.split(' ', 2)
        if len(f) == 3 and f[1] == path:
            return f[2]
    return ''


def foreign_region(path):
    base = os.path.join(ROOT, 'instances')
    for d in os.listdir(base):
        mf = os.path.join(base, d, 'manifest')
        if mf == p('manifest') or not os.path.isfile(mf):
            continue
        for line in read(mf).splitlines():
            if line.startswith('region ' + path + ' '):
                return True
    return False


def replace(path, text):
    tmp = path + '.rue-tmp'
    with open(tmp, 'w', encoding='utf-8') as f:
        f.write(text)
    os.replace(tmp, path)


def strip_region(path, anchor):
    if not os.path.isfile(path):
        return False
    lines = read(path).split('\n')
    begin = '# rue-region ' + anchor + ' begin'
    end = '# rue-region ' + anchor + ' end'
    if lines.count(begin) != 1 or lines.count(end) != 1:
        return False
    out = []
    skip = False
    for line in lines:
        if line == begin:
            skip = True
            continue
        if line == end:
            skip = False
            continue
        if not skip:
            out.append(line)
    replace(path, '\n'.join(out))
    return True


def region_set(path, anchor, content):
    strip_region(path, anchor)
    text = read(path) if os.path.isfile(path) else ''
    if text and not text.endswith('\n'):
        text += '\n'
    replace(path, text + '# rue-region ' + anchor + ' begin\n' + content + '\n# rue-region ' + anchor + ' end\n')


def restore(snapshot, path):
    shutil.copyfile(snapshot, path + '.rue-tmp')
    os.replace(path + '.rue-tmp', path)


def remove(path):
    if os.path.isfile(path):
        os.remove(path)


def write(path, content):
    replace(path, content)


def append(path, line):
    with open(path, 'a', encoding='utf-8') as f:
        f.write(line + '\n')


def run(command):
    subprocess.run(RUN + [command], check=False)


def defer(n):
    append(p('drift'), str(n))


def clobbered(n):
    append(p('clobbered'), str(n))

if os.path.exists(p('fired')):
    sys.exit(0)
now = int(time.time())
due = False
if os.path.isfile(p('deadline')) and now >= int(read(p('deadline')).strip()):
    due = True
if not due:
    sys.exit(0)

# step 1: winfw_allow
m = p('markers', '1')
if os.path.exists(m):
    skip = False
    if skip:
        defer(1)
    else:
        run('Remove-NetFirewallRule -Name rue-mgmt')
        os.remove(m)

open(p('fired'), 'w').close()
sys.exit(0)
