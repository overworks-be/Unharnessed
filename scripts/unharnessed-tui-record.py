#!/usr/bin/env python3
"""Records one conversation of the real binary over ACP, for the terminal
UI to replay (`unharnessed --tui-replay <recording>`).

usage: scripts/unharnessed-tui-record.py <out-dir> [recording]

The model is the faux provider (UNHARNESSED_FAUX_SCRIPT) at a reading
pace; its cells run in a real kernel, so the variables, the server left
running and its last lines are the kernel's own. Nothing reaches the
network. The agent runs in its own agent dir and HOME under <out-dir>,
where the kernel venv lives too; provider keys and tokens from the caller's
environment are dropped. While a command runs, its last lines are asked
for every second, and the start of each variable's value each time the
variable changes, as a panel showing them open does.

The recording (default <out-dir>/demo.out) has the format of the other
end-to-end traces: the command, then every frame sent (>) and received (<)
with the seconds since start. UNHARNESSED_BIN picks the binary (default
target/debug/unharnessed).
"""
import itertools
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = os.environ.get("UNHARNESSED_BIN", str(ROOT / "target/debug/unharnessed"))
OUT = Path(sys.argv[1]).resolve()
RECORDING = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else OUT / "demo.out"
PORT = 8765
TAIL_LINES = 7

PROMPT = "Serve the pages in site/ and tell me how heavy each one is."

PAGES = {
    "index.html": "<h1>Unharnessed</h1>\n" + "<p>One tool: a Python REPL.</p>\n" * 40,
    "docs.html": "<h1>Docs</h1>\n" + "<p>The kernel stays alive for the whole session.</p>\n" * 260,
    "pricing.html": "<h1>Pricing</h1>\n<p>Free.</p>\n",
}


def call(code):
    return {"type": "toolCall", "name": "ipython", "arguments": {"code": code}}


def thinking(text):
    return {"type": "thinking", "thinking": text}


def text(text):
    return {"type": "text", "text": text}


RESPONSES = [
    {"content": [
        thinking("The pages are in site/. Before serving anything I want to see what is there: "
                 "the file names and their sizes on disk, kept in a variable so I do not read the folder twice.\n\n"
                 "Then a server in the background, and one request per page to measure what actually goes over the wire."),
        call('from pathlib import Path\n\n'
             'pages = sorted(Path("site").glob("*.html"))\n'
             'sizes = {page.name: page.stat().st_size for page in pages}\n'
             'sizes'),
    ]},
    {"content": [
        text("Three pages. I am serving them in the background, then I will request each one."),
        call(f'server = bash("python3 -u -m http.server {PORT} --directory site", timeout=None)'),
    ]},
    {"content": [
        call('import time\n'
             'import urllib.request\n\n\n'
             'def fetch(name: str) -> tuple[int, float]:\n'
             '    """A page\'s bytes over HTTP, and the milliseconds it took."""\n'
             '    start = time.perf_counter()\n'
             f'    body = urllib.request.urlopen(f"http://localhost:{PORT}/{{name}}").read()\n'
             '    return len(body), round((time.perf_counter() - start) * 1000, 1)\n\n\n'
             'time.sleep(2.5)  # the server binds its port\n'
             'timings = {name: fetch(name) for name in sizes}\n'
             'timings'),
    ]},
    {"content": [
        call('heaviest = max(sizes, key=sizes.get)\n'
             'budget = 10_000\n'
             'assert sizes[heaviest] <= budget, f"{heaviest} is {sizes[heaviest]:,} bytes, over the {budget:,} budget"'),
    ]},
    {"content": [
        thinking("docs.html is over the budget I set, by a lot: that is the answer, not an error to fix. "
                 "The other two are small. I will give the sizes, say which one is heavy and why, "
                 "and leave the server running since the user asked for it."),
        text("The server is up on `localhost:" + str(PORT) + "` and still running.\n\n"
             "## Page weights\n\n"
             "- **docs.html** is the heavy one: about 14 KB, well over a 10 KB budget. "
             "It repeats the same paragraph 260 times.\n"
             "- **index.html** is 1.3 KB.\n"
             "- **pricing.html** is 30 bytes.\n\n"
             "Every page came back in a few milliseconds, so size is the only thing to act on. "
             "The numbers are still in the kernel:\n\n"
             "```python\n"
             "sizes[heaviest]   # bytes on disk\n"
             "timings[heaviest] # (bytes received, milliseconds)\n"
             "```\n\n"
             "| Page | On disk | Share |\n"
             "|:--|--:|--:|\n"
             "| docs.html | 13,794 B | 91% |\n"
             "| index.html | 1,301 B | 9% |\n"
             "| pricing.html | 30 B | under 1% |\n\n"
             "Tell me if you want `docs.html` split into sections."),
    ]},
]

SCRIPT = {
    "responses": RESPONSES,
    "reasoning": True,
    "tokensPerSecond": 30,
    "modelName": "Faux Model",
    "cost": {"input": 3, "output": 15, "cacheRead": 0.3, "cacheWrite": 3.75},
}


def main():
    base = OUT / "demo"
    shutil.rmtree(base, ignore_errors=True)
    agent_dir, home, work = base / "agent", base / "home", base / "work"
    for folder in (agent_dir, home, work / "site"):
        folder.mkdir(parents=True, exist_ok=True)
    for name, body in PAGES.items():
        (work / "site" / name).write_text(body)
    env = {
        name: value
        for name, value in os.environ.items()
        if not name.endswith(("_API_KEY", "_TOKEN")) and not name.startswith("UNHARNESSED_")
    }
    env.update(
        UNHARNESSED_KERNEL_VENV=str(OUT / "kernel-venv"),
        UNHARNESSED_OFFLINE="1",
        DO_NOT_TRACK="1",
        UNHARNESSED_CODING_AGENT_DIR=str(agent_dir),
        HOME=str(home),
        UNHARNESSED_FAUX_SCRIPT=json.dumps(SCRIPT),
    )
    RECORDING.parent.mkdir(parents=True, exist_ok=True)
    out = open(RECORDING, "w")
    out.write(f"$ cd {work} && UNHARNESSED_FAUX_SCRIPT=<scripts/unharnessed-tui-record.py> unharnessed --mode acp\n")
    child = subprocess.Popen(
        [BIN, "--mode", "acp"], cwd=work, text=True, env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(base / "stderr.log", "w"))
    start = time.time()
    lock = threading.Lock()
    arrived = threading.Condition(lock)
    frames = []
    ids = itertools.count(1)
    running = set()
    variables = {}

    def log(arrow, frame):
        with lock:
            out.write(f"{time.time() - start:7.3f} {arrow} {json.dumps(frame)}\n")
            out.flush()

    def read():
        for line in child.stdout:
            if not line.strip():
                continue
            frame = json.loads(line)
            log("<", frame)
            with lock:
                frames.append(frame)
                if frame.get("method") == "_unharnessed/kernel/changed":
                    commands = frame["params"].get("commands")
                    if commands is not None:
                        running.clear()
                        running.update(c["id"] for c in commands if c["status"] == "running")
                    listed = frame["params"].get("variables")
                    if listed is not None:
                        variables.clear()
                        variables.update((v["name"], v["rev"]) for v in listed)
                arrived.notify_all()

    def send(method, params):
        request_id = next(ids)
        frame = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        log(">", frame)
        with lock:
            child.stdin.write(json.dumps(frame) + "\n")
            child.stdin.flush()
        return request_id

    def answer(request_id, timeout=300):
        deadline = time.time() + timeout
        with lock:
            while True:
                found = next((f for f in frames if f.get("id") == request_id and "method" not in f), None)
                if found is not None:
                    return found
                if time.time() > deadline:
                    raise TimeoutError(f"no answer to request {request_id}")
                arrived.wait(timeout=0.2)

    threading.Thread(target=read, daemon=True).start()
    answer(send("initialize", {"protocolVersion": 1, "clientCapabilities": {}}))
    session = answer(send("session/new", {"cwd": str(work), "mcpServers": []}))["result"]["sessionId"]

    done = threading.Event()

    def tails():
        while not done.wait(1.0):
            with lock:
                ids_now = sorted(running)
            for command in ids_now:
                send("_unharnessed/kernel/tail", {"sessionId": session, "id": command, "lines": TAIL_LINES})

    def previews():
        """Each variable's value, asked again when the variable changes; while a cell runs the kernel
        refuses (busy), and it is asked again."""
        shown = {}
        while not done.wait(0.3):
            with lock:
                changed = [(name, rev) for name, rev in variables.items() if shown.get(name) != rev]
            for name, rev in changed:
                reply = answer(send("_unharnessed/kernel/preview", {"sessionId": session, "name": name}), timeout=30)
                if "busy" in json.dumps(reply.get("error", "")):
                    break
                shown[name] = rev

    threading.Thread(target=tails, daemon=True).start()
    threading.Thread(target=previews, daemon=True).start()
    reply = answer(send("session/prompt", {"sessionId": session, "prompt": [{"type": "text", "text": PROMPT}]}))
    # The panel goes on showing the server a moment after the answer.
    time.sleep(3.0)
    done.set()
    answer(send("session/close", {"sessionId": session}))
    child.stdin.close()
    code = child.wait(timeout=60)
    out.close()
    received = sum(1 for _ in frames)
    print(f"{RECORDING}: {received} frames received, prompt answered {json.dumps(reply.get('result') or reply.get('error'))}, exit {code}")
    return 0 if code == 0 and reply.get("result", {}).get("stopReason") == "end_turn" else 1


if __name__ == "__main__":
    sys.exit(main())
