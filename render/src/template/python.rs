//! The Python template: any OS, run by `uv run --offline --script` with
//! PEP 723 inline metadata; standard library only. A `run` body is launched
//! through the host's shell: `sh -c` on POSIX, `powershell.exe -Command` on
//! Windows.

use rue_core::artifact::Shell;
use rue_core::model::Drift;

use super::banner;
use crate::actions::{Action, Kind};
use crate::quote::python;
use crate::{Context, RenderError};

fn q(step: u32, s: &str) -> Result<String, RenderError> {
    python(s).map_err(|inner| RenderError::Unquotable { step, inner })
}

pub fn render(ctx: &Context<'_>) -> Result<String, RenderError> {
    let mut o = String::new();
    o.push_str("#!/usr/bin/env -S uv run --script\n");
    o.push_str("# /// script\n# requires-python = \">=3.11\"\n# dependencies = []\n# ///\n");
    o.push_str(&format!("# {}\n", banner(ctx)));
    o.push_str(
        "import hashlib\nimport os\nimport shutil\nimport subprocess\nimport sys\nimport time\n\n",
    );
    o.push_str(&format!("ROOT = {}\n", q(0, &ctx.root)?));
    o.push_str(&format!("INSTANCE = {}\n", q(0, &ctx.instance.id)?));
    o.push_str("INST = os.path.join(ROOT, 'instances', INSTANCE)\n");
    let prefix = match ctx.shell {
        Shell::Posix => "['sh', '-c']",
        Shell::Powershell => "['powershell.exe', '-NoProfile', '-NonInteractive', '-Command']",
    };
    o.push_str(&format!("RUN = {prefix}\n\n"));
    o.push_str(HELPERS);
    o.push_str(
        "\nif os.path.exists(p('fired')):\n    sys.exit(0)\nnow = int(time.time())\ndue = False\n",
    );
    if ctx.deadline {
        o.push_str("if os.path.isfile(p('deadline')) and now >= int(read(p('deadline')).strip()):\n    due = True\n");
    }
    if let Some(hb) = ctx.heartbeat_s {
        o.push_str(&format!("if not os.path.isfile(p('heartbeat')) or now - int(read(p('heartbeat')).strip()) > {hb}:\n    due = True\n"));
    }
    o.push_str("if not due:\n    sys.exit(0)\n\n");
    for s in &ctx.steps {
        let n = s.n;
        o.push_str(&format!("# step {}: {}\n", n, s.id));
        o.push_str(&format!(
            "m = p('markers', '{n}')\nif os.path.exists(m):\n    skip = False\n"
        ));
        for f in &s.files {
            if f.kind == Kind::Region {
                continue;
            }
            let pth = q(n, &f.path)?;
            match s.drift {
                Drift::Defer => o.push_str(&format!(
                    "    if sha({pth}) != recorded(m, {pth}):\n        skip = True\n"
                )),
                Drift::Clobber => o.push_str(&format!(
                    "    if sha({pth}) != recorded(m, {pth}):\n        clobbered({n})\n"
                )),
            }
        }
        o.push_str(&format!("    if skip:\n        defer({n})\n    else:\n"));
        for a in &s.actions {
            o.push_str("        ");
            match a {
                Action::Remove { path } => o.push_str(&format!("remove({})\n", q(n, path)?)),
                Action::StripRegion { path, anchor, k } => {
                    let pth = q(n, path)?;
                    let fallback = match s.drift {
                        Drift::Clobber => format!(
                            "if foreign_region({pth}):\n                defer({n})\n            else:\n                restore(p('snapshots', '{n}', '{k}'), {pth})\n                clobbered({n})"
                        ),
                        Drift::Defer => format!("defer({n})"),
                    };
                    o.push_str(&format!(
                        "if not strip_region({pth}, {}):\n            {fallback}\n",
                        q(n, anchor)?
                    ));
                }
                Action::RestoreSnapshot { path, k } => o.push_str(&format!(
                    "restore(p('snapshots', '{n}', '{k}'), {})\n",
                    q(n, path)?
                )),
                Action::Run { command } => o.push_str(&format!("run({})\n", q(n, command)?)),
                Action::Write { path, content } => {
                    o.push_str(&format!("write({}, {})\n", q(n, path)?, q(n, content)?))
                }
                Action::Append { path, line } => {
                    o.push_str(&format!("append({}, {})\n", q(n, path)?, q(n, line)?))
                }
                Action::RegionSet {
                    path,
                    anchor,
                    content,
                } => o.push_str(&format!(
                    "region_set({}, {}, {})\n",
                    q(n, path)?,
                    q(n, anchor)?,
                    q(n, content)?
                )),
                Action::RegionClear { path, anchor } => o.push_str(&format!(
                    "strip_region({}, {})\n",
                    q(n, path)?,
                    q(n, anchor)?
                )),
            }
        }
        o.push_str("        os.remove(m)\n\n");
    }
    o.push_str("open(p('fired'), 'w').close()\nsys.exit(0)\n");
    Ok(o)
}

const HELPERS: &str = r#"
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
"#;
