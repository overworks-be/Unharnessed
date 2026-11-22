#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyte"]
# ///
"""End-to-end checks of the terminal install (install.sh, and the archive
scripts/unharnessed-package.sh makes of the real binary), each on a machine
of its own: a HOME under <out-dir> and a PATH that holds only the tools the
installer needs.

usage: uv run scripts/unharnessed-install-e2e.py <out-dir>

How an install can fail, which is what the checks are for:
- the agent, reached through its link, does not find what ships beside it
  (macOS names the link, not the file): its skills, its kernel's runtime;
- an archive that is not the release's is installed: changed on the way, cut
  short, of another version than asked, no Unharnessed build, or a build
  that does not run here;
- a refused install leaves something behind, or takes the version that
  worked away;
- an update removes the version a running session uses, or never removes
  any; it removes a folder that is not its own;
- it overwrites a file of the user's where its link goes, or writes into
  the user's shell files;
- the wrong archive is asked for: the platform (a terminal under Rosetta),
  the release's address;
- a script cut short by a failed download runs half of itself;
- it needs a shell that is not the system's `sh` (dash on Linux);
- uv missing stops the install, or is not said; with uv, the kernel is not
  built, or is built from another runtime than the installed one;
- after two updates the kernel's skills still point into the removed
  version;
- uninstalling takes the user's conversations and keys with it, or a link
  that is not the installer's.

The archives are made of UNHARNESSED_BIN (default target/release/unharnessed)
under versions of the test's own, and served by an HTTP server on this
machine; the release's address is only read from what the installer asks a
stand-in `curl` for. Nothing reaches the network but uv, when its cache
lacks a package of the kernel's (the caller's cache is used). The session
that ends the checks runs in a pseudo-terminal with the faux provider.
It writes <out-dir>/install.out: each command and what it printed. Exits
non-zero when a check fails.
"""
import fcntl
import functools
import hashlib
import http.server
import json
import os
import pty
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tarfile
import termios
import threading
import time
from pathlib import Path

import pyte

ROOT = Path(__file__).resolve().parent.parent
BIN = os.environ.get("UNHARNESSED_BIN", str(ROOT / "target/release/unharnessed"))
OUT = Path(sys.argv[1]).resolve()
OUT.mkdir(parents=True, exist_ok=True)
INSTALL = ROOT / "install.sh"
FAILED = []
TRACE = open(OUT / "install.out", "w")
# What install.sh runs, and what the tools it runs run (tar's gzip, shasum's perl is named whole).
TOOLS = ("sh", "dash", "bash", "curl", "tar", "gzip", "shasum", "sha256sum", "uname", "sysctl", "cut", "sed",
         "mkdir", "rmdir", "rm", "mv", "ln", "cp", "cat", "dirname", "basename", "readlink", "head")
SYSTEM = "/usr/bin:/bin:/usr/sbin:/sbin"

# What the faux model answers: a cell that needs the kernel's own skills, then a sentence.
SCRIPT = {
    "responses": [
        {"content": [{"type": "toolCall", "name": "ipython",
                      "arguments": {"code": "import goal, attach_image\ntotal = sum(range(10))\ntotal"}}]},
        {"content": [{"type": "text", "text": "The total is 45."}]},
    ],
    "tokensPerSecond": 400,
    "modelName": "Faux Model",
}


def note(text):
    TRACE.write(text.rstrip("\n") + "\n")
    TRACE.flush()


def check(name, ok, detail=""):
    print(("PASS  " if ok else "FAIL  ") + name + ("" if ok or detail == "" else f" ({detail})"))
    note(("PASS  " if ok else "FAIL  ") + name)
    if not ok:
        FAILED.append(name)


def packaged(version, binary=BIN):
    """The folder holding the archive of `binary` under `version`, and the platform it is named by."""
    folder = OUT / "releases" / version
    shutil.rmtree(folder, ignore_errors=True)
    env = dict(os.environ, UNHARNESSED_BIN=str(binary), UNHARNESSED_PACKAGE_VERSION=version)
    note(f"$ UNHARNESSED_BIN={binary} UNHARNESSED_PACKAGE_VERSION={version} scripts/unharnessed-package.sh '' {folder}")
    # No target named: this machine's.
    done = subprocess.run([str(ROOT / "scripts/unharnessed-package.sh"), "", str(folder)], env=env,
                          capture_output=True, text=True)
    note(done.stdout + done.stderr + f"[exit {done.returncode}]")
    if done.returncode != 0:
        sys.exit(f"the archive of {version} was not made:\n{done.stdout}{done.stderr}")
    return folder


class Machine:
    """A HOME of its own and a PATH of the installer's tools alone; `uv` puts uv among them."""

    def __init__(self, name, uv=False):
        self.name = name
        self.base = OUT / f"install-{name}"
        shutil.rmtree(self.base, ignore_errors=True)
        self.home = self.base / "home"
        self.stubs = self.base / "stubs"
        self.tools = self.base / "tools"
        for folder in (self.home, self.stubs, self.tools):
            folder.mkdir(parents=True)
        for tool in TOOLS + (("uv",) if uv else ()):
            found = shutil.which(tool)
            if found:
                (self.tools / tool).symlink_to(found)
        self.root = self.home / ".local/share/unharnessed"
        self.bin = self.home / ".local/bin"
        self.link = self.bin / "unharnessed"
        self.env = {
            "HOME": str(self.home),
            "PATH": f"{self.stubs}:{self.tools}",
            "SHELL": "/bin/zsh",
            "TERM": "xterm-256color",
            "COLORTERM": "truecolor",
            "DO_NOT_TRACK": "1",
        }
        if uv:
            # The caller's downloads, so that a HOME of its own does not fetch Python and the packages again.
            for name, asked in (("UV_CACHE_DIR", ["cache", "dir"]), ("UV_PYTHON_INSTALL_DIR", ["python", "dir"])):
                self.env[name] = subprocess.run(["uv", *asked], capture_output=True, text=True).stdout.strip()

    def stub(self, name, body):
        """A program of the test's that comes first on PATH."""
        path = self.stubs / name
        path.write_text("#!/bin/sh\n" + body + "\n")
        path.chmod(0o755)

    def install(self, *args, source=None, shell="sh", piped=True, script=None, **env):
        """install.sh run as its users run it (read by the shell from a pipe); its exit code and what it printed."""
        env = {**self.env, **{name: str(value) for name, value in env.items()}}
        if source is not None:
            env["UNHARNESSED_INSTALL_FROM"] = str(source)
        told = " ".join(f"{name}={value}" for name, value in env.items()
                        if name.startswith("UNHARNESSED_") or name in ("SHELL",))
        text = INSTALL.read_bytes() if script is None else script
        if piped:
            command = [str(self.tools / shell), "-s", "--", *args]
            note(f"$ {told} {shell} -s -- {' '.join(args)} < install.sh" + ("" if script is None else
                                                                          f"   # its first {len(script)} bytes"))
            done = subprocess.run(command, input=text, env=env, cwd=self.home, capture_output=True, timeout=600)
        else:
            command = [str(self.tools / shell), str(INSTALL), *args]
            note(f"$ {told} {shell} install.sh {' '.join(args)}")
            done = subprocess.run(command, env=env, cwd=self.home, capture_output=True, timeout=600)
        printed = (done.stdout + done.stderr).decode(errors="replace")
        note(printed + f"[exit {done.returncode}]")
        return done.returncode, printed

    def run(self, *args, **env):
        """The installed agent through its link; its exit code and what it printed."""
        env = {**self.env, "PATH": f"{self.env['PATH']}:{SYSTEM}", **{name: str(value) for name, value in env.items()}}
        note(f"$ {self.link} {' '.join(args)}")
        done = subprocess.run([str(self.link), *args], env=env, cwd=self.home, capture_output=True, text=True,
                              timeout=300)
        note(done.stdout + done.stderr + f"[exit {done.returncode}]")
        return done.returncode, done.stdout + done.stderr

    def versions(self):
        """The versions installed: the folders that hold an agent."""
        if not self.root.is_dir():
            return []
        return sorted(folder.name for folder in self.root.iterdir() if (folder / "unharnessed").is_file())

    def linked(self):
        """The version the link names, None without a link of the installer's."""
        if not self.link.is_symlink():
            return None
        to = Path(os.readlink(self.link))
        return to.parent.name if to.name == "unharnessed" and to.parent.parent == self.root else None

    def left(self):
        """Everything under HOME, as paths from it, without what the versions hold."""
        found = set()
        for path in self.home.rglob("*"):
            inside = path.relative_to(self.home)
            if len(inside.parts) > 5 and inside.parts[:3] == (".local", "share", "unharnessed"):
                continue
            found.add(str(inside))
        return found


class Served:
    """An HTTP server on this machine over the archives' folder."""

    def __init__(self, folder):
        handler = functools.partial(Quiet, directory=str(folder))
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


class Terminal:
    """A program in a pseudo-terminal, its output read by a terminal emulator."""

    def __init__(self, program, env, cwd, columns=120, rows=36):
        self.screen = pyte.Screen(columns, rows)
        self.stream = pyte.ByteStream(self.screen)
        note(f"$ cd {cwd} && {program}   # in a {columns}x{rows} pseudo-terminal, the faux provider answering")
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(cwd)
            os.execve(program, [program], env)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))

    def read(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            ready, _, _ = select.select([self.fd], [], [], 0.05)
            if not ready:
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError:
                return
            if not data:
                return
            # What turns the kitty keyboard protocol on and off, which the emulator would print.
            self.stream.feed(re.sub(rb"\x1b\[[<>]1u", b"", data))

    def text(self):
        return "\n".join(row.rstrip() for row in self.screen.display)

    def waits(self, words, label, timeout=60.0):
        end = time.time() + timeout
        while words not in self.text() and time.time() < end:
            self.read(0.1)
        self.read(0.4)
        note(f"-- the screen, {label}:\n" + self.text())
        return words in self.text()

    def keys(self, keys, label):
        note(f"-- keys: {label}")
        os.write(self.fd, keys.encode())

    def exit(self, timeout=60.0):
        end = time.time() + timeout
        while time.time() < end:
            self.read(0.05)
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                code = os.waitstatus_to_exitcode(status)
                note(f"[exit {code}]")
                return code
        os.kill(self.pid, signal.SIGKILL)
        os.waitpid(self.pid, 0)
        note("[killed: it did not leave]")
        return None


def asked(machine):
    """What the stand-in curl was asked for: the addresses."""
    log = machine.base / "curl.log"
    return re.findall(r"https?://\S+", log.read_text()) if log.exists() else []


def stand_in_curl(machine):
    machine.stub("curl", f'echo "$@" >> "{machine.base / "curl.log"}"\nexit 22')


def scenario_archive(state):
    folder = state["folders"]["0.0.90"]
    archives = sorted(folder.glob("unharnessed-*.tar.gz"))
    check("archive: one archive is made, named by this machine's platform, with its SHA-256 beside it",
          len(archives) == 1 and re.fullmatch(r"unharnessed-(darwin|linux)-(arm64|x64)\.tar\.gz", archives[0].name)
          and (folder / (archives[0].name + ".sha256")).is_file(), str([path.name for path in folder.iterdir()]))
    archive = archives[0]
    state["archive"] = archive.name
    state["target"] = archive.name.removeprefix("unharnessed-").removesuffix(".tar.gz")
    said = (folder / (archive.name + ".sha256")).read_text().split()
    check("archive: the checksum beside it is its own, and names it",
          said == [hashlib.sha256(archive.read_bytes()).hexdigest(), archive.name], str(said))
    with tarfile.open(archive) as tar:
        members = {member.name: member for member in tar.getmembers()}
        manifest = json.load(tar.extractfile("unharnessed/package.json"))
    check("archive: everything is in one folder, with no file of macOS's own beside the files",
          all(name == "unharnessed" or name.startswith("unharnessed/") for name in members)
          and not any(Path(name).name.startswith("._") for name in members), str(sorted(members)[:6]))
    check("archive: it holds the agent, executable, the version it says, the kernel's runtime and the built-in skills",
          members["unharnessed/unharnessed"].mode & 0o111 == 0o111 and manifest["version"] == "0.0.90"
          and "unharnessed/unharnessed-runtime/pyproject.toml" in members
          and "unharnessed/unharnessed-runtime/src/rlm/repl.py" in members
          and all(f"unharnessed/skills/{skill}/SKILL.md" in members for skill in ("goal", "mcp", "attach-image"))
          and "unharnessed/LICENSE" in members)
    tracked = subprocess.run(["git", "ls-files", "--", "unharnessed-runtime/pyproject.toml", "unharnessed-runtime/src",
                              "skills"], cwd=ROOT, capture_output=True, text=True).stdout.split("\n")
    shipped = {name.removeprefix("unharnessed/") for name, member in members.items() if member.isfile()}
    check("archive: of the runtime and the skills, the files git tracks are shipped, and no other",
          shipped - {"unharnessed", "package.json", "LICENSE"} == set(filter(None, tracked)),
          str(sorted(shipped ^ set(filter(None, tracked)))[:8]))


def scenario_install(state):
    served = state["served"]
    for shell in ("sh", "dash", "bash"):
        machine = Machine(f"first-{shell}")
        if not (machine.tools / shell).exists():
            note(f"-- no {shell} on this machine")
            continue
        code, printed = machine.install("--no-kernel", source=f"{served.url}/0.0.90", shell=shell)
        check(f"install ({shell}): downloaded over HTTP, the version is in its folder and the link names it",
              code == 0 and machine.versions() == ["0.0.90"] and machine.linked() == "0.0.90"
              and "Unharnessed 0.0.90 is installed in" in printed, f"{code} {machine.versions()} {machine.linked()}")
    machine = Machine("first")
    code, printed = machine.install(source=f"{served.url}/0.0.90")
    check("install: without uv it still installs, and says how to get uv and that the kernel comes at the first run",
          code == 0 and machine.linked() == "0.0.90" and "curl -LsSf https://astral.sh/uv/install.sh | sh" in printed
          and "first time the agent runs Python" in printed and not (machine.home / ".unharnessed").exists(), printed)
    check("install: the link's folder not on PATH, it says the line that adds it, for the user's shell",
          f"{machine.bin} is not on your PATH" in printed
          and "echo 'export PATH=\"$HOME/.local/bin:$PATH\"' >> ~/.zshrc" in printed, printed)
    check("install: it wrote the version's folder and the link, and nothing else under HOME",
          machine.left() == {".local", ".local/bin", ".local/bin/unharnessed", ".local/share",
                             ".local/share/unharnessed", ".local/share/unharnessed/0.0.90",
                             ".local/share/unharnessed/0.0.90/unharnessed",
                             ".local/share/unharnessed/0.0.90/package.json",
                             ".local/share/unharnessed/0.0.90/LICENSE",
                             ".local/share/unharnessed/0.0.90/skills",
                             ".local/share/unharnessed/0.0.90/unharnessed-runtime"}, str(sorted(machine.left())))
    code, said = machine.run("--version")
    check("install: through the link, the agent says the release's version", code == 0 and said.strip() == "0.0.90", said)
    code, prompt = machine.run("prompt", UNHARNESSED_OFFLINE="1")
    places = re.findall(r"<location>(.*?)</location>", prompt)
    inside = str((machine.root / "0.0.90" / "skills").resolve())
    check("install: through the link, the agent finds its built-in skills in the installed folder",
          code == 0 and len(places) >= 3 and all(str(Path(place).resolve()).startswith(inside) for place in places),
          str(places))
    code, printed = machine.install("--no-kernel", source=f"{served.url}/0.0.90",
                                    PATH=f"{machine.env['PATH']}:{machine.bin}")
    check("install: run again on the same version, it says so; the folder on PATH, it says how to run it",
          code == 0 and "Unharnessed 0.0.90 is installed again" in printed and "not on your PATH" not in printed
          and "unharnessed" in printed.splitlines()[-1] and machine.linked() == "0.0.90", printed)
    code, printed = machine.install("--no-kernel", source=state["folders"]["0.0.90"], piped=False, SHELL="/usr/bin/fish")
    check("install: from a folder and run as a file, it installs too; a fish user is told fish's line",
          code == 0 and machine.linked() == "0.0.90" and f"fish_add_path {machine.bin}" in printed, printed)
    code, printed = machine.install("--help")
    check("install: --help says the options and installs nothing new",
          code == 0 and "--uninstall" in printed and "UNHARNESSED_INSTALL_FROM" in printed, printed)


def scenario_update(state):
    served = state["served"]
    machine = Machine("update")
    machine.install("--no-kernel", source=f"{served.url}/0.0.90")
    (machine.root / "notes").mkdir()
    (machine.root / "notes" / "mine.txt").write_text("not the installer's\n")
    code, printed = machine.install("--no-kernel", source=f"{served.url}/0.0.91")
    check("update: the link names the new version, and the one it named is kept for the sessions running on it",
          code == 0 and machine.linked() == "0.0.91" and machine.versions() == ["0.0.90", "0.0.91"]
          and "Unharnessed 0.0.91 is installed" in printed and "(it was 0.0.90)" in printed,
          f"{machine.linked()} {machine.versions()} {printed}")
    code, said = machine.run("--version")
    check("update: the agent says the new version", said.strip() == "0.0.91", said)
    code, printed = machine.install("--no-kernel", source=f"{served.url}/0.0.92")
    check("update: at the next one the oldest version goes, and a folder that is not the installer's stays",
          code == 0 and machine.linked() == "0.0.92" and machine.versions() == ["0.0.91", "0.0.92"]
          and (machine.root / "notes" / "mine.txt").is_file(), f"{machine.linked()} {machine.versions()}")
    code, printed = machine.install("--no-kernel", "--version", "0.0.91", source=f"{served.url}/0.0.92")
    check("update: an archive of another version than the one asked for is refused, the link unchanged",
          code != 0 and "asked for version 0.0.91" in printed and machine.linked() == "0.0.92"
          and machine.versions() == ["0.0.91", "0.0.92"], printed)
    code, printed = machine.install("--no-kernel", source=f"{served.url}/0.0.91", UNHARNESSED_VERSION="v0.0.91")
    check("update: an older version asked for by name is installed (going back)",
          code == 0 and machine.linked() == "0.0.91" and machine.versions() == ["0.0.91", "0.0.92"], printed)
    check("update: nothing of the downloads is left in the install's folder",
          not [path.name for path in machine.root.iterdir() if path.name.startswith(".")],
          str([path.name for path in machine.root.iterdir()]))


def changed(state, name, change):
    """A folder with the archive of 0.0.92 after `change(folder, archive)`."""
    folder = OUT / "releases" / name
    shutil.rmtree(folder, ignore_errors=True)
    shutil.copytree(state["folders"]["0.0.92"], folder)
    change(folder, folder / state["archive"])
    return folder


def resummed(folder, archive):
    (folder / (archive.name + ".sha256")).write_text(
        f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")


def other_archive(folder, archive, make):
    """The archive replaced by one of `make(app)`'s folder, with a checksum that is its own."""
    app = folder / "stage" / "unharnessed"
    app.mkdir(parents=True)
    make(app)
    archive.unlink()
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(app, "unharnessed")
    shutil.rmtree(folder / "stage")
    resummed(folder, archive)


def not_a_build(app):
    (app / "README").write_text("something else\n")


def does_not_run(app):
    (app / "unharnessed").write_text("#!/bin/sh\necho 'cannot execute binary file' >&2\nexit 126\n")
    (app / "unharnessed").chmod(0o755)
    (app / "unharnessed-runtime").mkdir()
    (app / "unharnessed-runtime" / "pyproject.toml").write_text("")
    (app / "skills").mkdir()


def scenario_refusals(state):
    served = state["served"]
    nothing = {".local", ".local/bin", ".local/share"}

    def refused(name, folder, words):
        machine = Machine(f"refused-{name}")
        code, printed = machine.install("--no-kernel", source=folder)
        return code != 0 and words in printed and machine.left() <= nothing, f"{code} {printed} {machine.left()}"

    def one_byte_more(folder, archive):
        with open(archive, "ab") as file:
            file.write(b"\0")

    def cut_short(folder, archive):
        archive.write_bytes(archive.read_bytes()[: archive.stat().st_size // 2])

    for name, change, words in (
        ("changed", one_byte_more, "is not the one the release published"),
        ("cut", cut_short, "is not the one the release published"),
        ("no-checksum", lambda folder, archive: (folder / (archive.name + ".sha256")).unlink(), "no checksum at"),
        ("empty-checksum", lambda folder, archive: (folder / (archive.name + ".sha256")).write_text(""),
         "is not the one the release published"),
    ):
        ok, detail = refused(name, f"{served.url}/{changed(state, name, change).name}", words)
        check(f"refusals: an archive {name.replace('-', ' ')} is refused, and nothing is installed", ok, detail)
    ok, detail = refused("other", changed(state, "other", lambda folder, archive: other_archive(folder, archive, not_a_build)),
                         "is not an Unharnessed build")
    check("refusals: an archive of something else, its checksum right, is refused, and nothing is installed", ok, detail)
    ok, detail = refused("alien", changed(state, "alien", lambda folder, archive: other_archive(folder, archive, does_not_run)),
                         "does not run on this machine")
    check("refusals: a build that does not run here is refused with what it said, and nothing is installed",
          ok and "cannot execute binary file" in detail, detail)
    ok, detail = refused("absent", f"{served.url}/0.0.404", f"the latest release has no terminal build for {state['target']}")
    check("refusals: a release without the archive is said so, and nothing is installed", ok, detail)

    machine = Machine("refused-update")
    machine.install("--no-kernel", source=f"{served.url}/0.0.90")
    code, printed = machine.install("--no-kernel", source=f"{served.url}/changed")
    check("refusals: a refused update leaves the version that worked, its link, and nothing of its own",
          code != 0 and machine.linked() == "0.0.90" and machine.versions() == ["0.0.90"]
          and machine.run("--version")[1].strip() == "0.0.90"
          and not [path.name for path in machine.root.iterdir() if path.name.startswith(".")], printed)

    machine = Machine("refused-file")
    machine.bin.mkdir(parents=True)
    machine.link.write_text("the user's own\n")
    code, printed = machine.install("--no-kernel", source=f"{served.url}/0.0.90")
    check("refusals: a file of the user's where the link goes is not replaced, and nothing is installed",
          code != 0 and "is a file this installer did not put there" in printed
          and machine.link.read_text() == "the user's own\n" and machine.versions() == [], printed)

    machine = Machine("refused-option")
    code, printed = machine.install("--force")
    check("refusals: an option it does not know is refused", code != 0 and "unknown option --force" in printed
          and machine.left() == set(), printed)

    script = INSTALL.read_bytes()
    left = {}
    for size in (len(script) // 4, len(script) // 2, len(script) * 3 // 4, len(script) - 40, len(script) - 8,
                 len(script) - 2):
        machine = Machine(f"refused-cut-{size}")
        machine.install("--no-kernel", source=f"{served.url}/0.0.90", script=script[:size])
        left[size] = machine.left()
    check("refusals: a script cut short, wherever, installs nothing", not any(left.values()), str(left))


def scenario_platforms(state):
    for name, system, machine_name, translated, wanted in (
        ("apple silicon", "Darwin", "arm64", "0", "darwin-arm64"),
        ("an Intel Mac", "Darwin", "x86_64", "0", "darwin-x64"),
        ("a terminal under Rosetta", "Darwin", "x86_64", "1", "darwin-arm64"),
        ("Linux on x86", "Linux", "x86_64", "", "linux-x64"),
        ("Linux on ARM", "Linux", "aarch64", "", "linux-arm64"),
    ):
        machine = Machine(f"platform-{wanted}-{translated}")
        machine.stub("uname", f'case "$1" in -s) echo {system} ;; -m) echo {machine_name} ;; esac')
        machine.stub("sysctl", f'[ -n "{translated}" ] && echo {translated}')
        stand_in_curl(machine)
        machine.install("--no-kernel")
        check(f"platforms: {name} asks the latest release for the {wanted} archive",
              asked(machine)[:1] == [f"https://github.com/unharnessed-agent/Unharnessed/releases/latest/download/"
                                     f"unharnessed-{wanted}.tar.gz"], str(asked(machine)))
    machine = Machine("platform-version")
    stand_in_curl(machine)
    code, printed = machine.install("--no-kernel", "--version", "1.2.3")
    check("platforms: a version asked for is that release's archive, and its absence names the release",
          asked(machine)[:1] == [f"https://github.com/unharnessed-agent/Unharnessed/releases/download/vscode-v1.2.3/"
                                 f"{state['archive']}"]
          and code != 0 and f"release 1.2.3 has no terminal build for {state['target']}" in printed, printed)
    for name, system, machine_name, words in (
        ("Windows", "MINGW64_NT-10.0", "x86_64", "no terminal build for Windows yet"),
        ("another system", "FreeBSD", "amd64", "no build for FreeBSD"),
        ("another processor", "Linux", "riscv64", "no build for linux on riscv64"),
    ):
        machine = Machine(f"platform-{machine_name}-{system[:5]}")
        machine.stub("uname", f'case "$1" in -s) echo {system} ;; -m) echo {machine_name} ;; esac')
        stand_in_curl(machine)
        code, printed = machine.install("--no-kernel")
        check(f"platforms: {name} is refused before anything is downloaded",
              code != 0 and words in printed and asked(machine) == [] and machine.left() == set(), printed)


def runtime_source(machine):
    """Where the kernel's runtime was installed from, as its venv recorded it."""
    found = list((machine.home / ".unharnessed/kernel-venv").rglob("unharnessed_runtime-*.dist-info/direct_url.json"))
    return json.loads(found[0].read_text())["url"] if found else None


def session(machine, label):
    """One message in the installed terminal UI; whether its cell ran in the kernel, and how the UI left."""
    env = {**machine.env, "PATH": f"{machine.env['PATH']}:{SYSTEM}", "UNHARNESSED_FAUX_SCRIPT": json.dumps(SCRIPT),
           "UNHARNESSED_OFFLINE": "1"}
    work = machine.base / "work"
    work.mkdir(exist_ok=True)
    term = Terminal(str(machine.link), env, work)
    opened = term.waits("Ask Unharnessed", f"{label}: opened")
    term.keys("what is the total?\r", "a message, Enter")
    answered = term.waits("The total is 45.", f"{label}: answered", timeout=180.0)
    # The kernel's panel lists the cell's variable: the cell ran, its imports with it.
    ran = re.search(r"total\s+int\b", term.text()) is not None
    term.keys("\x11", "Ctrl+Q")
    return opened, answered, ran, term.exit()


def scenario_kernel(state):
    served = state["served"]
    if not shutil.which("uv"):
        check("kernel: uv is on this machine (the kernel's checks need it)", False)
        return
    machine = Machine("kernel", uv=True)
    code, printed = machine.install(source=f"{served.url}/0.0.90")
    python = machine.home / ".unharnessed/kernel-venv/bin/python"
    check("kernel: with uv, the install builds the Python kernel", code == 0 and "Building the Python kernel" in printed
          and "not built yet" not in printed and python.exists(), printed)
    source = runtime_source(machine)
    inside = (machine.root / "0.0.90" / "unharnessed-runtime").resolve()
    check("kernel: its runtime is the installed one",
          source is not None and Path(source.removeprefix("file://")).resolve() == inside, str(source))
    opened, answered, ran, left = session(machine, "the installed version")
    check("kernel: the installed agent opens its terminal UI, runs a cell on the kernel's skills, and answers",
          opened and answered and ran, f"{opened} {answered} {ran}")
    check("kernel: the terminal UI leaves cleanly", left == 0, str(left))
    machine.install(source=f"{served.url}/0.0.91")
    code, printed = machine.install(source=f"{served.url}/0.0.92")
    check("kernel: two updates later the first version is gone", code == 0 and machine.versions() == ["0.0.91", "0.0.92"],
          str(machine.versions()))
    opened, answered, ran, left = session(machine, "two updates later")
    check("kernel: the agent still runs a cell on the kernel's skills, which the removed version held",
          opened and answered and ran and left == 0, f"{opened} {answered} {ran} {left}")

    sessions = list((machine.home / ".unharnessed").rglob("*.jsonl"))
    other = Machine("uninstall-other")
    other.bin.mkdir(parents=True)
    other.link.symlink_to("/bin/sh")
    code, printed = other.install("--uninstall")
    check("uninstall: a link that is not the installer's is left", code == 0 and other.link.is_symlink()
          and "left as it is" in printed, printed)
    code, printed = machine.install("--uninstall")
    check("uninstall: the versions and the link are removed",
          code == 0 and not machine.root.exists() and not machine.link.is_symlink() and not machine.link.exists(), printed)
    check("uninstall: the conversations and the kernel stay, and it says so",
          len(sessions) >= 1 and all(path.exists() for path in sessions) and python.exists()
          and "they are kept" in printed, f"{sessions} {printed}")


def main():
    if not Path(BIN).is_file():
        sys.exit(f"no binary at {BIN}: cargo build --release -p uh-cli, or set UNHARNESSED_BIN")
    state = {"folders": {version: packaged(version) for version in ("0.0.90", "0.0.91", "0.0.92")}}
    state["served"] = Served(OUT / "releases")
    only = os.environ.get("UNHARNESSED_E2E_ONLY")
    for scenario in (scenario_archive, scenario_install, scenario_update, scenario_refusals, scenario_platforms,
                     scenario_kernel):
        if only and scenario.__name__ not in ("scenario_archive", f"scenario_{only}"):
            continue
        scenario(state)
    print(f"\n{len(FAILED)} failed" if FAILED else "\nall passed")
    sys.exit(1 if FAILED else 0)


main()
