#!/usr/bin/env python3

from __future__ import annotations

import argparse
import base64
import html
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

MACOS = Path(__file__).resolve().parents[2]
ROOT = MACOS.parent
CLI_DIR = ROOT / "cli"
EXAMPLES = ROOT / "example-docs"
TRUNCATE = 6000
DEFAULT = object()

GIT_ENV = {
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_AUTHOR_NAME": "Jim",
    "GIT_AUTHOR_EMAIL": "jim@example.com",
    "GIT_COMMITTER_NAME": "Jim",
    "GIT_COMMITTER_EMAIL": "jim@example.com",
}

ID_PATTERN = re.compile(r"[0-9a-f]{8}")
PNG_1X1 = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==")


class Config:
    def __init__(self) -> None:
        self.marq = Path(os.environ.get("MARQ_BIN") or MACOS / ".build" / "debug" / "marq")
        self.pdftool = Path(os.environ.get("PDFTOOL_BIN") or MACOS / ".build" / "debug" / "pdftool")
        self.cli = Path(os.environ.get("MARQ_COMMENTS_BIN") or CLI_DIR / "target" / "debug" / "marq-comments")
        self.out = MACOS / ".harness" / "comments-acceptance"
        self.no_bundle = False
        self.skip_real = False
        self.new_delay = 6.0
        self.new_settle = 14.0
        self.hang_limit = 20.0


CFG = Config()


class ScenarioFailed(Exception):
    pass


class ScenarioSkipped(Exception):
    pass


@dataclass
class Build:
    ok: bool
    reason: str
    log: str = ""


def run_build(command: list[str], cwd: Path, label: str, timeout: int = 1800) -> Build:
    try:
        proc = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    except FileNotFoundError:
        return Build(False, f"{command[0]} is not installed")
    except subprocess.TimeoutExpired:
        return Build(False, f"{label} took over {timeout} seconds")
    log = (proc.stdout + proc.stderr)[-TRUNCATE:]
    if proc.returncode != 0:
        return Build(False, f"{label} failed (exit {proc.returncode})", log)
    return Build(True, f"{label} built", log)


def build_all(skip: bool) -> list[Build]:
    builds = []
    if skip:
        for name, path in (("marq", CFG.marq), ("pdftool", CFG.pdftool), ("marq-comments", CFG.cli)):
            builds.append(Build(path.exists(), f"--no-build: {name} at {path}" if path.exists()
                                else f"--no-build and {name} does not exist at {path}"))
        return builds
    builds.append(run_build(["swift", "build"], MACOS, "swift build"))
    if os.environ.get("MARQ_COMMENTS_BIN"):
        found = CFG.cli.exists()
        builds.append(Build(found, f"using MARQ_COMMENTS_BIN={CFG.cli}" if found
                            else f"MARQ_COMMENTS_BIN={CFG.cli} does not exist"))
    elif (CLI_DIR / "Cargo.toml").exists():
        builds.append(run_build(["cargo", "build"], CLI_DIR, "cargo build"))
    else:
        builds.append(Build(False, "cli/Cargo.toml does not exist"))
    for path in (CFG.marq, CFG.pdftool, CFG.cli):
        if not path.exists():
            builds.append(Build(False, f"{path} does not exist after the build"))
    return builds


@dataclass
class Event:
    kind: str
    text: str = ""
    ok: bool = True
    detail: str = ""
    code: int = 0
    out: str = ""
    err: str = ""
    where: str = ""
    file: str = ""


class Ctx:
    def __init__(self, slug: str):
        self.slug = slug
        self.events: list[Event] = []
        self.tmp = Path(tempfile.mkdtemp(prefix=f"marq-comments-{slug}-"))
        self.shots = 0

    def note(self, text: str) -> None:
        self.events.append(Event("note", text))

    def check(self, description: str, condition: object, detail: str = "") -> None:
        self.events.append(Event("check", description, bool(condition), detail))
        if not condition:
            raise ScenarioFailed(description + (f" ({detail})" if detail else ""))

    def cleanup(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)


def child_env(bin_env: object = DEFAULT, extra: dict[str, str] | None = None) -> dict[str, str]:
    env = {**os.environ, **GIT_ENV}
    if bin_env is DEFAULT:
        env["MARQ_COMMENTS_BIN"] = str(CFG.cli)
    elif bin_env is None:
        env.pop("MARQ_COMMENTS_BIN", None)
    else:
        env["MARQ_COMMENTS_BIN"] = str(bin_env)
    if extra:
        env.update(extra)
    return env


def shown_command(argv: list[str]) -> str:
    return " ".join(a if re.fullmatch(r"[\w./:=@,-]+", a) else repr(a) for a in argv)


def run_proc(c: Ctx, argv: list[str], *, cwd: Path, env: dict[str, str], timeout: float, where: str,
             expect: tuple[int, ...] = (0,), label: str = "") -> tuple[subprocess.CompletedProcess, float]:
    shown = label or shown_command([Path(argv[0]).name, *argv[1:]])
    start = time.time()
    try:
        proc = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=timeout, env=env)
    except FileNotFoundError:
        c.events.append(Event("run", shown, False, "the program does not exist", -1, where=where))
        raise ScenarioFailed(f"{argv[0]} does not exist: build it first")
    except subprocess.TimeoutExpired:
        c.events.append(Event("run", shown, False, f"timed out after {timeout:g} seconds", -1, where=where))
        raise ScenarioFailed(f"timed out after {timeout:g} seconds: {shown}")
    elapsed = time.time() - start
    ok = proc.returncode in expect
    c.events.append(Event("run", shown, ok, f"{elapsed:.2f}s", proc.returncode,
                          proc.stdout if len(proc.stdout) < 2000 else "", proc.stderr[-1500:], where))
    if not ok:
        raise ScenarioFailed(f"exit code {proc.returncode}, expected {expect[0]}: {shown}")
    return proc, elapsed


def parse_json(c: Ctx, text: str, what: str) -> object:
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        c.check(f"{what} is valid JSON", False, f"{error}: {text[:120]!r}")
        raise AssertionError


class Repo:
    def __init__(self, ctx: Ctx, name: str, path: Path, doc: str):
        self.ctx = ctx
        self.name = name
        self.path = path
        self.doc = doc

    @staticmethod
    def create(ctx: Ctx, name: str, files: dict[str, bytes], doc: str, git: bool = True) -> "Repo":
        path = ctx.tmp / name
        path.mkdir(parents=True)
        repo = Repo(ctx, name, path, doc)
        for relative, data in files.items():
            target = path / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        if git:
            repo.git("init", "-q", "-b", "main")
            repo.git("config", "user.name", "Jim")
            repo.git("config", "user.email", "jim@example.com")
            repo.git("add", ".")
            repo.git("commit", "-q", "-m", "Add the fixture")
        return repo

    def git(self, *args: str) -> str:
        proc = subprocess.run(["git", *args], cwd=self.path, capture_output=True, text=True, env=child_env(None))
        if proc.returncode != 0:
            raise RuntimeError(f"git {' '.join(args)} failed in {self.name}: {proc.stderr.strip()}")
        return proc.stdout

    @property
    def file(self) -> Path:
        return self.path / self.doc

    def read(self) -> str:
        return self.file.read_text(encoding="utf-8")

    def write(self, text: str) -> None:
        self.file.write_text(text, encoding="utf-8")

    def edit(self, old: str, new: str) -> None:
        text = self.read()
        self.ctx.check(f"setup: {old[:40]!r} is in the markdown exactly once", text.count(old) == 1,
                       f"found {text.count(old)}")
        self.write(text.replace(old, new))
        self.ctx.note(f"{self.name}: {old[:50]!r} became {new[:50]!r}")

    def find(self, word: str) -> tuple[int, int]:
        text = self.read()
        index = text.find(word)
        self.ctx.check(f"setup: {word!r} is in the markdown", index >= 0)
        line = text.count("\n", 0, index) + 1
        return line, index

    def line_text(self, line: int) -> str:
        return self.read().split("\n")[line - 1]

    def cli(self, *args: str, agent: bool = False, author: str | None = None, expect: int = 0) -> str:
        argv = [str(CFG.cli)]
        if agent:
            argv.append("--agent")
        if author:
            argv += ["--author", author]
        argv += list(args)
        proc, _ = run_proc(self.ctx, argv, cwd=self.path, env=child_env(), timeout=60, where=self.name,
                           expect=(expect,), label="marq-comments " + shown_command(list(argv[1:])))
        return proc.stdout

    def new_id(self, out: str, what: str) -> str:
        short = out.strip()
        self.ctx.check(f"{what} prints an 8-character id on one line", ID_PATTERN.fullmatch(short) is not None,
                       repr(short))
        return short

    def comment_word(self, word: str, message: str = "A comment.", nth: int | None = None, doc: str | None = None) -> str:
        line, _ = self.find(word)
        args = ["comment", doc or self.doc, "--line", str(line), "--text", word]
        if nth:
            args += ["--nth", str(nth)]
        return self.new_id(self.cli(*args, "-m", message), f"comment on {word!r}")

    def comment_line(self, needle: str, message: str = "A comment on a line.") -> tuple[str, str]:
        line, _ = self.find(needle)
        cid = self.new_id(self.cli("comment", self.doc, "--line", str(line), "-m", message), "line comment")
        return cid, self.line_text(line)

    def comment_range(self, first: str, last: str, message: str = "A comment on a range.") -> tuple[str, str]:
        text = self.read()
        self.ctx.check(f"setup: {first!r} and {last!r} are in the markdown", first in text and last in text)
        start = text.index(first)
        end = text.index(last) + len(last)
        cid = self.new_id(self.cli("comment", self.doc, "--range", f"{start}:{end}", "-m", message), "range comment")
        return cid, text[start:end]

    def reply(self, cid: str, message: str) -> str:
        return self.new_id(self.cli("reply", cid, "-m", message, agent=True, author="Claude <noreply@anthropic.com>"),
                           "reply")

    def suggest(self, word: str, replacement: str) -> str:
        line, _ = self.find(word)
        return self.new_id(self.cli("suggest", self.doc, "--line", str(line), "--text", word, "--replace",
                                    replacement, "-m", "A suggestion."), "suggestion")

    def threads(self) -> list[dict]:
        data = parse_json(self.ctx, self.cli("list", self.doc, "--json"), "list --json")
        self.ctx.check("list --json prints an array", isinstance(data, list), type(data).__name__)
        return data  # type: ignore[return-value]

    def thread(self, short: str) -> dict:
        for t in self.threads():
            if t["annotation"]["id"].replace("urn:uuid:", "").startswith(short):
                return t
        self.ctx.check(f"list --json includes thread {short}", False)
        raise AssertionError


FIXTURE = """# Heading headword

Plain paragraph with plainword in it, then **a boldword here**, then `a codeword here`, then a [linkword here](https://example.com) end.

- First item with itemword
- Second item with other words

| Name | Value |
|---|---|
| cellword | 42 |
| other | 7 |

Whole line sentence is marked entirely.

First block of the range ends at rangestart.

The second block of the range begins at rangeend.

Close one: alpha beta gamma delta.

Reply target line with replyword.

Edit line with changeword here.

Orphan line with orphanword here.

Suggest line with suggestword here.

Delete line with deleteword here.

Closing paragraph of the fixture.
"""

FIX_DOC = "docs/fixture.md"
BIG_DOC = "example-docs/test.md"


def fixture_files() -> dict[str, bytes]:
    return {FIX_DOC: FIXTURE.encode("utf-8")}


def big_files() -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    for path in sorted(EXAMPLES.rglob("*")):
        if path.is_file():
            files[f"example-docs/{path.relative_to(EXAMPLES).as_posix()}"] = path.read_bytes()
    logo = MACOS / "assets" / "logo.svg"
    if logo.exists():
        files["macos/assets/logo.svg"] = logo.read_bytes()
    return files


def fixture_repo(c: Ctx, name: str = "work") -> Repo:
    return Repo.create(c, name, fixture_files(), FIX_DOC)


def big_repo(c: Ctx, name: str = "work") -> Repo:
    return Repo.create(c, name, big_files(), BIG_DOC)


def marq_argv(repo: Repo, *, width: int | None, comments: str | None, numbers: str | None, click: str | None,
              print_mode: bool, output: tuple[str, str], settle: float | None, timeout: int) -> list[str]:
    argv = [str(CFG.marq), str(repo.file)]
    if width:
        argv += ["--width", str(width)]
    flag = {"metrics": "--dump-metrics", "png": "--export-png", "pdf": "--export-pdf"}[output[0]]
    argv += [flag, output[1]]
    if print_mode:
        argv.append("--print")
    if comments:
        argv += ["--comments", comments]
    if numbers:
        argv += ["--comment-numbers", numbers]
    if click:
        argv += ["--comments-click", click]
    if settle is not None:
        argv += ["--settle", str(settle)]
    argv += ["--timeout", str(timeout), "--harness-run"]
    return argv


def run_marq(c: Ctx, repo: Repo, *, width: int | None = 1400, comments: str | None = None,
             numbers: str | None = None, click: str | None = None, print_mode: bool = False,
             bin_env: object = DEFAULT, timeout: int = 60, limit: float = 150, label: str = "") -> dict:
    argv = marq_argv(repo, width=width, comments=comments, numbers=numbers, click=click, print_mode=print_mode,
                     output=("metrics", "-"), settle=None, timeout=timeout)
    proc, elapsed = run_proc(c, argv, cwd=MACOS, env=child_env(bin_env), timeout=limit, where=repo.name,
                             label=label or "marq " + shown_command(argv[2:-3]))
    metrics = parse_json(c, proc.stdout, "the metrics dump")
    c.check("the metrics dump is an object", isinstance(metrics, dict))
    metrics["__seconds"] = elapsed
    return metrics


def shot(c: Ctx, repo: Repo, name: str = "", **flags: object) -> None:
    c.shots += 1
    file = f"{c.slug}{('-' + name) if name else ''}.png"
    target = CFG.out / "shots" / file
    target.parent.mkdir(parents=True, exist_ok=True)
    width = flags.pop("width", 1400)
    argv = marq_argv(repo, width=width, comments=flags.get("comments"), numbers=flags.get("numbers"),
                     click=flags.get("click"), print_mode=False, output=("png", str(target)), settle=None, timeout=60)
    bin_env = flags.get("bin_env", DEFAULT)
    try:
        subprocess.run(argv, cwd=MACOS, capture_output=True, text=True, timeout=150, env=child_env(bin_env))
    except (subprocess.TimeoutExpired, FileNotFoundError):
        c.note(f"no screenshot: {name or 'main'}")
        return
    if target.exists() and target.stat().st_size > 0:
        c.events.append(Event("shot", name or "window", True, file=f"shots/{file}", where=repo.name))
    else:
        c.note(f"no screenshot: {name or 'main'}")


def slim(m: dict) -> dict:
    block = m.get("comments")
    if not isinstance(block, dict):
        return {"comments": None}
    out = dict(block)
    if isinstance(out.get("gutter"), list):
        out["gutter"] = f"{len(out['gutter'])} entries"
    return out


def record(c: Ctx, label: str, m: dict) -> None:
    c.events.append(Event("metrics", label, True, json.dumps(slim(m), indent=1)[:TRUNCATE]))


TEMP_PATH = re.compile(r"marq-comments-[^/\"]+/[^/\"]+/")


CACHE_BUST = re.compile(r"\?t=\d+")


def strip(m: dict) -> dict:
    kept = {k: v for k, v in m.items() if k not in ("comments", "__seconds")}
    return json.loads(CACHE_BUST.sub("", TEMP_PATH.sub("TEMP/", json.dumps(kept))))


def gutter_of(m: dict):
    block = m.get("comments")
    if isinstance(block, dict) and isinstance(block.get("gutter"), list):
        return block["gutter"]
    if isinstance(m.get("gutter"), list):
        return m["gutter"]
    return None


def diffs(a: object, b: object, path: str = "", out: list[str] | None = None, limit: int = 5) -> list[str]:
    out = [] if out is None else out
    if len(out) >= limit:
        return out
    if isinstance(a, dict) and isinstance(b, dict):
        for key in sorted(set(a) | set(b)):
            if key not in a:
                out.append(f"{path}.{key} added")
            elif key not in b:
                out.append(f"{path}.{key} removed")
            else:
                diffs(a[key], b[key], f"{path}.{key}", out, limit)
    elif isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            out.append(f"{path}: {len(a)} entries -> {len(b)}")
        for i, (x, y) in enumerate(zip(a, b)):
            diffs(x, y, f"{path}[{i}]", out, limit)
    elif a != b:
        out.append(f"{path}: {a!r} -> {b!r}")
    return out[:limit]


def comments_of(c: Ctx, m: dict, label: str = "") -> dict:
    block = m.get("comments")
    c.check(f"{label + ': ' if label else ''}the metrics carry a comments block", isinstance(block, dict),
            "keys: " + ", ".join(sorted(k for k in m if k != "__seconds")))
    return block


def thread_of(c: Ctx, block: dict, short: str) -> dict:
    threads = block.get("threads")
    c.check("the comments block lists threads", isinstance(threads, list))
    for t in threads:
        if str(t.get("id", "")).replace("urn:uuid:", "").startswith(short):
            return t
    c.check(f"the comments block lists thread {short}", False, str([t.get("id") for t in threads]))
    raise AssertionError


def squash(text: str) -> str:
    return re.sub(r"\s+", "", text)


def mark_tops(block: dict) -> list[float]:
    return [mark["top"] for t in block["threads"] for mark in t.get("marks") or []]


def equal_metrics(c: Ctx, description: str, a: dict, b: dict) -> None:
    found = diffs(strip(a), strip(b))
    c.check(description, not found, "; ".join(found))


SCENARIOS: list[tuple[str, str, str, object, bool]] = []


def scenario(slug: str, title: str, expectation: str, real_only: bool = False):
    def register(fn):
        SCENARIOS.append((slug, title, expectation, fn, real_only))
        return fn
    return register


def single_word(c: Ctx, word: str, expected: str, geometry: bool = True) -> None:
    r = fixture_repo(c)
    cid = r.comment_word(word)
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, word, m)
    t = thread_of(c, block, cid)
    c.check("the anchor is anchored", t.get("status") == "anchored", str(t.get("status")))
    c.check("the thread has one mark", len(t.get("marks") or []) == 1, str(len(t.get("marks") or [])))
    c.check(f"the marked text is {expected!r}", t.get("markedText") == expected, repr(t.get("markedText")))
    if geometry:
        c.check("the mapper reports ok", block.get("mapper") == "ok", str(block.get("mapper")))
        c.check("the marked text equals the anchor text", t.get("anchorText") == t.get("markedText"),
                f"{t.get('anchorText')!r} vs {t.get('markedText')!r}")
        card = t.get("card")
        c.check("the thread has a card", isinstance(card, dict))
        c.check("the layout at width 1400 is the rail", block.get("layout") == "rail" and card.get("inRail") is True,
                f"layout {block.get('layout')}, inRail {card.get('inRail')}")
        offset = card["top"] - t["marks"][0]["top"]
        c.check("the card top is within 4px of the mark top", abs(offset) <= 4, f"{offset:.1f}px")


@scenario("01-one-word", "A comment on one word",
          "One mark whose text is the anchor text, and a card in the rail level with it.")
def one_word(c: Ctx) -> None:
    single_word(c, "plainword", "plainword")


@scenario("02-whole-line", "A comment on a whole line",
          "The whole line is marked, and one card sits level with it.")
def whole_line(c: Ctx) -> None:
    r = fixture_repo(c)
    cid, text = r.comment_line("Whole line sentence")
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "whole line", m)
    t = thread_of(c, block, cid)
    c.check("the anchor is anchored", t.get("status") == "anchored", str(t.get("status")))
    c.check("the marked text is the whole line", squash(t.get("markedText") or "") == squash(text),
            repr(t.get("markedText")))
    c.check("the thread has a card in the rail", (t.get("card") or {}).get("inRail") is True)


@scenario("03-word-in-bold", "A word inside bold text", "The mark covers the rendered word.")
def word_in_bold(c: Ctx) -> None:
    single_word(c, "boldword", "boldword", geometry=False)


@scenario("04-word-in-code", "A word inside a code span", "The mark covers the rendered word.")
def word_in_code(c: Ctx) -> None:
    single_word(c, "codeword", "codeword", geometry=False)


@scenario("05-word-in-link", "A word inside a link", "The mark covers the rendered word.")
def word_in_link(c: Ctx) -> None:
    single_word(c, "linkword", "linkword", geometry=False)


@scenario("06-word-in-heading", "A word inside a heading", "The mark covers the rendered word.")
def word_in_heading(c: Ctx) -> None:
    single_word(c, "headword", "headword", geometry=False)


@scenario("07-word-in-list-item", "A word inside a list item", "The mark covers the rendered word.")
def word_in_list(c: Ctx) -> None:
    single_word(c, "itemword", "itemword", geometry=False)


@scenario("08-word-in-table-cell", "A word inside a table cell", "The mark covers the rendered word.")
def word_in_cell(c: Ctx) -> None:
    single_word(c, "cellword", "cellword", geometry=False)


@scenario("09-range-two-blocks", "A range across two blocks",
          "Marks in both paragraphs and one card.")
def range_two_blocks(c: Ctx) -> None:
    r = fixture_repo(c)
    cid, source = r.comment_range("rangestart.", "rangeend")
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "range", m)
    t = thread_of(c, block, cid)
    c.check("the anchor is anchored", t.get("status") == "anchored", str(t.get("status")))
    marks = t.get("marks") or []
    c.check("the thread has marks in both blocks", len(marks) >= 2, str(len(marks)))
    c.check("the marks are on different lines", len({round(mark["top"]) for mark in marks}) >= 2)
    c.check("the marked text is the range, whitespace aside", squash(t.get("markedText") or "") == squash(source),
            repr(t.get("markedText")))
    c.check("there is one thread and one card", block.get("threadCount") == 1 and isinstance(t.get("card"), dict),
            str(block.get("threadCount")))


@scenario("10-close-threads", "Two threads close together",
          "The cards do not overlap and the second sits below the first.")
def close_threads(c: Ctx) -> None:
    r = fixture_repo(c)
    first = r.comment_word("alpha", "First.")
    second = r.comment_word("gamma", "Second.")
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "close", m)
    a, b = thread_of(c, block, first), thread_of(c, block, second)
    c.check("both threads have a card", isinstance(a.get("card"), dict) and isinstance(b.get("card"), dict))
    c.check("the overlaps list is empty", block.get("overlaps") == [], str(block.get("overlaps")))
    c.check("the second card is below the first",
            b["card"]["top"] >= a["card"]["top"] + a["card"]["height"],
            f"first {a['card']['top']}+{a['card']['height']}, second {b['card']['top']}")
    c.check("the first card stays level with its mark", abs(a["card"]["top"] - a["marks"][0]["top"]) <= 4)


@scenario("11-reply", "A thread with a reply", "One card holds the reply.")
def reply(c: Ctx) -> None:
    r = fixture_repo(c)
    cid = r.comment_word("replyword")
    r.reply(cid, "An answer.")
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "reply", m)
    t = thread_of(c, block, cid)
    c.check("there is one thread", block.get("threadCount") == 1, str(block.get("threadCount")))
    c.check("the thread has one reply", t.get("replies") == 1, str(t.get("replies")))
    c.check("the thread has one card", isinstance(t.get("card"), dict))
    c.check("the marked text is the word", t.get("markedText") == "replyword", repr(t.get("markedText")))


@scenario("12-changed", "A changed anchor",
          "The changed text is marked, and the card carries the original quote.")
def changed(c: Ctx) -> None:
    r = fixture_repo(c)
    cid = r.comment_word("changeword")
    r.edit("changeword", "changewrd")
    t0 = r.thread(cid)
    c.check("the CLI reports the anchor changed", t0["anchor"]["status"] == "changed", json.dumps(t0["anchor"]))
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "changed", m)
    t = thread_of(c, block, cid)
    c.check("the thread status is changed", t.get("status") == "changed", str(t.get("status")))
    c.check("the changed text is marked", t.get("markedText") == "changewrd", repr(t.get("markedText")))
    c.check("the thread has a card", isinstance(t.get("card"), dict))
    if "cardText" in t:
        c.check("the card shows the original quote", "changeword" in str(t["cardText"]), str(t["cardText"])[:120])
    else:
        c.note("the original quote on the card is not asserted: design 9.7 has no field for the card text")


@scenario("13-orphaned", "An orphaned thread",
          "No mark in the document, and the thread is listed after it.")
def orphaned(c: Ctx) -> None:
    r = fixture_repo(c)
    cid = r.comment_word("orphanword")
    r.edit("Orphan line with orphanword here.", "Orphan line with here.")
    t0 = r.thread(cid)
    c.check("the CLI reports the anchor orphaned", t0["anchor"]["status"] == "orphaned", json.dumps(t0["anchor"]))
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "orphaned", m)
    t = thread_of(c, block, cid)
    c.check("the thread status is orphaned", t.get("status") == "orphaned", str(t.get("status")))
    c.check("the thread has no marks", not t.get("marks"), str(t.get("marks")))
    listed = [o.get("id") for o in block.get("orphans") or []]
    c.check("the thread is in the orphans section",
            any(str(i).replace("urn:uuid:", "").startswith(cid) for i in listed), str(listed))
    c.check("the orphan count is 1", block.get("orphanCount") == 1, str(block.get("orphanCount")))


@scenario("14-accepted-suggestion", "An accepted suggestion",
          "The replacement text is marked and the thread's state is accepted.")
def accepted_suggestion(c: Ctx) -> None:
    r = fixture_repo(c)
    sid = r.suggest("suggestword", "suggestion")
    r.cli("accept", sid)
    c.check("the markdown holds the replacement", "with suggestion here" in r.read())
    t0 = r.thread(sid)
    c.check("the CLI reports the thread accepted and anchored",
            t0["state"] == "accepted" and t0["anchor"]["status"] == "anchored", json.dumps(t0["anchor"]))
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "accepted", m)
    t = thread_of(c, block, sid)
    c.check("the thread state is accepted", t.get("state") == "accepted", str(t.get("state")))
    c.check("the replacement is marked", t.get("markedText") == "suggestion", repr(t.get("markedText")))
    c.check("the thread has a card", isinstance(t.get("card"), dict))


@scenario("15-applied-deletion", "An applied deletion",
          "No mark, and the thread is listed after the document with status applied.")
def applied_deletion(c: Ctx) -> None:
    r = fixture_repo(c)
    sid = r.suggest("deleteword", "")
    r.cli("accept", sid)
    t0 = r.thread(sid)
    c.check("the CLI reports the anchor applied", t0["anchor"]["status"] == "applied", json.dumps(t0["anchor"]))
    m = run_marq(c, r)
    shot(c, r)
    block = comments_of(c, m)
    record(c, "applied", m)
    t = thread_of(c, block, sid)
    c.check("the thread status is applied", t.get("status") == "applied", str(t.get("status")))
    c.check("the thread has no marks", not t.get("marks"), str(t.get("marks")))
    entries = [o for o in block.get("orphans") or [] if str(o.get("id")).replace("urn:uuid:", "").startswith(sid)]
    c.check("the thread is in the orphans section", len(entries) == 1, str(block.get("orphans")))
    c.check("the orphans entry has status applied", entries[0].get("status") == "applied", str(entries[0]))


@scenario("16-numbers", "Numbers on and off",
          "The card numbers and subscripts show and hide, and no mark, card or gutter entry moves.")
def numbers(c: Ctx) -> None:
    r = fixture_repo(c)
    r.comment_word("plainword")
    r.comment_word("boldword")
    cid, _ = r.comment_line("Whole line sentence")
    on = run_marq(c, r, numbers="on", label="marq numbers on")
    shot(c, r, "on", numbers="on")
    off = run_marq(c, r, numbers="off", label="marq numbers off")
    shot(c, r, "off", numbers="off")
    a, b = comments_of(c, on, "numbers on"), comments_of(c, off, "numbers off")
    record(c, "numbers on", on)
    record(c, "numbers off", off)
    c.check("numbers on reports numbers true", a.get("numbers") is True, str(a.get("numbers")))
    c.check("numbers off reports numbers false", b.get("numbers") is False, str(b.get("numbers")))
    c.check("every thread has a number with numbers on",
            all(isinstance(t.get("number"), int) for t in a["threads"]), str([t.get("number") for t in a["threads"]]))
    c.check("the gutter entries are identical", gutter_of(on) is not None and gutter_of(on) == gutter_of(off),
            "; ".join(diffs(gutter_of(on), gutter_of(off))))
    c.check("every mark rectangle top is unchanged", mark_tops(a) == mark_tops(b) and len(mark_tops(a)) >= 3,
            f"{mark_tops(a)} vs {mark_tops(b)}")
    c.check("every card top is unchanged",
            [(t.get("card") or {}).get("top") for t in a["threads"]] == [(t.get("card") or {}).get("top") for t in b["threads"]])
    c.check("the document height is unchanged", on.get("documentHeight") == off.get("documentHeight"),
            f"{on.get('documentHeight')} vs {off.get('documentHeight')}")


@scenario("17-comments-hidden", "Comments shown and hidden",
          "Hidden has no marks, and its metrics and gutter equal a run on a copy with no comments.")
def comments_hidden(c: Ctx) -> None:
    r = fixture_repo(c)
    r.comment_word("plainword")
    r.comment_line("Whole line sentence")
    r.comment_range("rangestart.", "rangeend")
    plain = fixture_repo(c, "plain")
    shown = run_marq(c, r, comments="show", label="marq comments show")
    hidden = run_marq(c, r, comments="hide", label="marq comments hide")
    base = run_marq(c, plain, comments="show", label="marq on a copy with no comments")
    shot(c, r, "shown", comments="show")
    shot(c, r, "hidden", comments="hide")
    record(c, "shown", shown)
    record(c, "hidden", hidden)
    s = comments_of(c, shown, "shown")
    c.check("shown has marks", sum(len(t.get("marks") or []) for t in s["threads"]) >= 3)
    h = hidden.get("comments")
    if isinstance(h, dict):
        c.check("hidden reports shown false", h.get("shown") is False, str(h.get("shown")))
        c.check("hidden has no marks", sum(len(t.get("marks") or []) for t in h.get("threads") or []) == 0
                and h.get("markedCount", 0) == 0, str(h.get("markedCount")))
        c.check("hidden has no rail", h.get("layout") in ("none", None), str(h.get("layout")))
    equal_metrics(c, "hidden metrics equal the run with no comments", hidden, base)
    g = gutter_of(shown)
    c.check("the shown gutter exists", g is not None)
    c.check("the hidden gutter equals the shown gutter", gutter_of(hidden) == g, "; ".join(diffs(g, gutter_of(hidden))))
    if gutter_of(base) is not None:
        c.check("the gutter equals the run with no comments", gutter_of(base) == g)


def pdf_pages(c: Ctx, repo: Repo, name: str, comments: str) -> int:
    target = c.tmp / f"{name}.pdf"
    argv = marq_argv(repo, width=None, comments=comments, numbers=None, click=None, print_mode=False,
                     output=("pdf", str(target)), settle=None, timeout=90)
    run_proc(c, argv, cwd=MACOS, env=child_env(), timeout=200, where=repo.name,
             label=f"marq export-pdf comments {comments} ({name})")
    c.check(f"{name}: the PDF exists", target.exists() and target.stat().st_size > 0)
    proc, _ = run_proc(c, [str(CFG.pdftool), "info", str(target)], cwd=MACOS, env=child_env(), timeout=60,
                       where=repo.name, label=f"pdftool info {name}.pdf")
    info = parse_json(c, proc.stdout, "pdftool info")
    c.check("pdftool reports a page count", isinstance(info.get("pageCount"), int), str(info)[:100])
    return info["pageCount"]


@scenario("18-pdf", "PDF with comments shown and hidden",
          "Hidden equals today's page count and print metrics. Shown has no broken word and no overflow.")
def pdf(c: Ctx) -> None:
    r = big_repo(c)
    r.comment_word("document")
    r.comment_line("This is a blockquote")
    r.comment_word("italic")
    plain = big_repo(c, "plain")
    shot(c, r)
    base_pages = pdf_pages(c, plain, "plain", "show")
    hidden_pages = pdf_pages(c, r, "hidden", "hide")
    shown_pages = pdf_pages(c, r, "shown", "show")
    c.note(f"pages: no comments {base_pages}, hidden {hidden_pages}, shown {shown_pages}")
    c.check("hidden has the page count of the run with no comments", hidden_pages == base_pages,
            f"{hidden_pages} vs {base_pages}")
    base_print = run_marq(c, plain, width=None, print_mode=True, comments="show", label="marq print, no comments")
    hidden_print = run_marq(c, r, width=None, print_mode=True, comments="hide", label="marq print, hidden")
    shown_print = run_marq(c, r, width=None, print_mode=True, comments="show", label="marq print, shown")
    record(c, "print shown", shown_print)
    equal_metrics(c, "hidden print metrics equal the run with no comments", hidden_print, base_print)
    problems = shown_print.get("problems") or {}
    c.check("shown reports no broken words", problems.get("brokenWords") == [], str(problems.get("brokenWords"))[:200])
    c.check("shown reports no overflow", problems.get("overflowing") == [], str(problems.get("overflowing"))[:200])
    c.check("shown carries a comments block", isinstance(shown_print.get("comments"), dict))


@scenario("19-bundle", "The bundled app finds its own CLI",
          "With an empty PATH, Marq.app reports comments from the marq-comments it carries.", real_only=True)
def bundle(c: Ctx) -> None:
    app = MACOS / "build" / "Marq.app" / "Contents" / "MacOS" / "marq"
    if CFG.no_bundle:
        raise ScenarioSkipped(f"--no-bundle given, so the bundle is not rebuilt ({'present' if app.exists() else 'absent'} at {app})")
    run_build(["just", "bundle"], MACOS, "just bundle", 1800)
    c.check("just bundle produced the app", app.exists(), str(app))
    r = fixture_repo(c)
    cid = r.comment_word("plainword")
    argv = ["env", "-i", f"HOME={os.environ.get('HOME', '')}", str(app), str(r.file), "--dump-metrics", "-",
            "--width", "1400", "--timeout", "60", "--harness-run"]
    proc, _ = run_proc(c, argv, cwd=MACOS, env={"HOME": os.environ.get("HOME", "")}, timeout=150, where=r.name,
                       label="env -i HOME=... Marq.app/Contents/MacOS/marq FILE --dump-metrics -")
    m = parse_json(c, proc.stdout, "the metrics dump")
    block = comments_of(c, m)
    record(c, "bundle", m)
    c.check("the bundled app marked the thread", thread_of(c, block, cid).get("markedText") == "plainword")


@scenario("20-click", "A click on a mark and on a card",
          "A click on a mark makes its thread active, and a click on a card does the same.")
def click(c: Ctx) -> None:
    r = fixture_repo(c)
    first = r.comment_word("plainword")
    second = r.comment_word("replyword")
    base = run_marq(c, r, label="marq, no click")
    block = comments_of(c, base)
    c.check("no thread is active before a click", block.get("active") in (None, ""), str(block.get("active")))
    ids = {short: thread_of(c, block, short)["id"] for short in (first, second)}
    on_mark = run_marq(c, r, click=f"mark:{ids[first]}", label="marq click mark")
    shot(c, r, "mark", click=f"mark:{ids[first]}")
    b = comments_of(c, on_mark, "mark click")
    record(c, "click on a mark", on_mark)
    c.check("a click on a mark makes its thread active", b.get("active") == ids[first], str(b.get("active")))
    on_card = run_marq(c, r, click=f"card:{ids[second]}", label="marq click card")
    shot(c, r, "card", click=f"card:{ids[second]}")
    d = comments_of(c, on_card, "card click")
    record(c, "click on a card", on_card)
    c.check("a click on a card makes its thread active", d.get("active") == ids[second], str(d.get("active")))
    for key in ("cardActive", "marksActive", "active"):
        flagged = [t for t in d["threads"] if t.get(key) is True]
        if key != "active" and any(key in t for t in d["threads"]):
            c.check(f"only the clicked thread has {key}", [t["id"] for t in flagged] == [ids[second]], str(flagged))


@scenario("21-gutter", "The gutter with comments on and off, on every fixture",
          "The gutter entries are identical shown, hidden and with numbers, for each document.")
def gutter(c: Ctx) -> None:
    r = fixture_repo(c, "small")
    r.comment_word("plainword")
    r.comment_word("cellword")
    r.comment_range("rangestart.", "rangeend")
    b = big_repo(c, "big")
    b.comment_word("document")
    b.comment_word("italic")
    b.comment_line("This is a blockquote")
    for repo in (r, b):
        shown = run_marq(c, repo, comments="show", numbers="on", label=f"marq {repo.name} shown, numbers on")
        hidden = run_marq(c, repo, comments="hide", label=f"marq {repo.name} hidden")
        record(c, f"{repo.name} shown", shown)
        comments_of(c, shown, repo.name)
        g = gutter_of(shown)
        c.check(f"{repo.name}: the gutter has entries", isinstance(g, list) and len(g) > 0)
        c.check(f"{repo.name}: shown and hidden gutters are identical", g == gutter_of(hidden),
                "; ".join(diffs(g, gutter_of(hidden))))
    shot(c, r)


def baseline_check(c: Ctx, repo: Repo, plain: Repo, *, bin_env: object, expect_note: str) -> dict:
    base = run_marq(c, plain, bin_env=DEFAULT, label="marq baseline, a copy with no comments")
    m = run_marq(c, repo, bin_env=bin_env, label=f"marq: {expect_note}")
    shot(c, repo, bin_env=bin_env)
    record(c, expect_note, m)
    equal_metrics(c, "the metrics equal the plain baseline", m, base)
    block = m.get("comments")
    if isinstance(block, dict):
        c.check("any comments block shows nothing marked", block.get("markedCount", 0) == 0
                and block.get("layout") in ("none", None), f"markedCount {block.get('markedCount')}, layout {block.get('layout')}")
    return m


@scenario("22-no-comments", "A file with no comments", "The render equals the plain baseline.")
def no_comments(c: Ctx) -> None:
    r = fixture_repo(c)
    plain = fixture_repo(c, "plain")
    baseline_check(c, r, plain, bin_env=DEFAULT, expect_note="no comments in the repository")


@scenario("23-no-git", "A file outside a git repository", "The render equals the plain baseline.")
def no_git(c: Ctx) -> None:
    r = Repo.create(c, "loose", fixture_files(), FIX_DOC, git=False)
    plain = fixture_repo(c, "plain")
    baseline_check(c, r, plain, bin_env=DEFAULT, expect_note="a file with no git repository")


@scenario("24-no-cli", "No marq-comments binary", "The render equals the plain baseline.")
def no_cli(c: Ctx) -> None:
    r = fixture_repo(c)
    r.comment_word("plainword")
    plain = fixture_repo(c, "plain")
    baseline_check(c, r, plain, bin_env=c.tmp / "missing" / "marq-comments", expect_note="MARQ_COMMENTS_BIN at a missing path")


@scenario("25-cli-hangs", "A CLI that hangs",
          "The run finishes inside the watchdog and the render equals the plain baseline.")
def cli_hangs(c: Ctx) -> None:
    r = fixture_repo(c)
    r.comment_word("plainword")
    plain = fixture_repo(c, "plain")
    fake = c.tmp / "hang" / "marq-comments"
    fake.parent.mkdir()
    fake.write_text("#!/bin/sh\nexec sleep 30\n")
    fake.chmod(fake.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    m = baseline_check(c, r, plain, bin_env=fake, expect_note="a CLI that sleeps 30 seconds")
    c.check(f"the run finished in under {CFG.hang_limit:g} seconds", m["__seconds"] < CFG.hang_limit,
            f"{m['__seconds']:.1f}s")


@scenario("26-empty-bin", "An empty MARQ_COMMENTS_BIN", "Comments are off and the render equals the plain baseline.")
def empty_bin(c: Ctx) -> None:
    r = fixture_repo(c)
    r.comment_word("plainword")
    plain = fixture_repo(c, "plain")
    baseline_check(c, r, plain, bin_env="", expect_note="MARQ_COMMENTS_BIN empty")


@scenario("27-new-comment", "A comment added while marq runs",
          "A second process adds a comment during a long settle, and the card is in the final metrics.")
def new_comment(c: Ctx) -> None:
    r = fixture_repo(c)
    existing = r.comment_word("plainword", "Already here.")
    out = c.tmp / "metrics.json"
    argv = marq_argv(r, width=1400, comments=None, numbers=None, click=None, print_mode=False,
                     output=("metrics", str(out)), settle=CFG.new_settle, timeout=int(CFG.new_settle + 60))
    c.note(f"method: a headless run with --settle {CFG.new_settle:g}; a second process adds the comment "
           f"{CFG.new_delay:g} seconds after launch. The comment is written to md-comments and the markdown "
           "file is untouched, so only the ref watch can refresh the page.")
    proc = subprocess.Popen(argv, cwd=MACOS, env=child_env(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        time.sleep(CFG.new_delay)
        added = r.comment_word("replyword", "Added while running.")
        before = r.file.stat().st_mtime_ns
        try:
            stdout, stderr = proc.communicate(timeout=CFG.new_settle + 90)
        except subprocess.TimeoutExpired:
            proc.kill()
            c.check("marq finished", False, "no exit within the settle and timeout")
    finally:
        if proc.poll() is None:
            proc.kill()
    c.events.append(Event("run", "marq (background)", proc.returncode == 0, "", proc.returncode, "", stderr[-1500:], r.name))
    c.check("marq exited 0", proc.returncode == 0, str(proc.returncode))
    c.check("the markdown file was not touched", r.file.stat().st_mtime_ns == before)
    c.check("marq wrote the metrics", out.exists())
    m = parse_json(c, out.read_text(), "the metrics file")
    block = comments_of(c, m)
    record(c, "after the new comment", m)
    c.check("the existing thread is still there", thread_of(c, block, existing).get("markedText") == "plainword")
    t = thread_of(c, block, added)
    c.check("the new comment has a mark and a card", t.get("markedText") == "replyword" and isinstance(t.get("card"), dict),
            repr(t.get("markedText")))
    c.check("there are two threads", block.get("threadCount") == 2, str(block.get("threadCount")))


@scenario("28-narrow", "A narrow window",
          "At width 700 the layout is stacked, each card sits under its block, and the gutter is unchanged.")
def narrow(c: Ctx) -> None:
    r = fixture_repo(c)
    cid = r.comment_word("plainword")
    r.comment_word("replyword")
    shown = run_marq(c, r, width=700, comments="show", label="marq width 700 shown")
    hidden = run_marq(c, r, width=700, comments="hide", label="marq width 700 hidden")
    shot(c, r, width=700)
    block = comments_of(c, shown)
    record(c, "width 700", shown)
    c.check("the layout is stacked", block.get("layout") == "stacked", str(block.get("layout")))
    t = thread_of(c, block, cid)
    card, mark = t.get("card") or {}, (t.get("marks") or [{}])[0]
    c.check("the card is not in the rail", card.get("inRail") is False, str(card.get("inRail")))
    c.check("the card is below its mark", card.get("top", -1) >= mark.get("top", 0) + mark.get("height", 0),
            f"card {card.get('top')}, mark {mark.get('top')}+{mark.get('height')}")
    c.check("the gutter is unchanged", gutter_of(shown) is not None and gutter_of(shown) == gutter_of(hidden),
            "; ".join(diffs(gutter_of(shown), gutter_of(hidden))))


@scenario("29-just-check", "just check", "The existing baselines are unchanged.", real_only=True)
def just_check(c: Ctx) -> None:
    proc, _ = run_proc(c, ["just", "check"], cwd=MACOS, env={**os.environ}, timeout=1800, where="macos",
                       label="just check")
    c.note(proc.stdout[-1500:])


@dataclass
class Result:
    slug: str
    title: str
    expectation: str
    status: str
    reason: str
    events: list[Event]
    seconds: float


def run_scenario(slug: str, title: str, expectation: str, fn) -> Result:
    ctx = Ctx(slug)
    start = time.time()
    status, reason = "PASS", ""
    try:
        fn(ctx)
    except ScenarioSkipped as skipped:
        status, reason = "SKIP", str(skipped)
    except ScenarioFailed as failure:
        status, reason = "FAIL", str(failure)
    except Exception as error:
        status, reason = "FAIL", f"harness error: {type(error).__name__}: {error}"
    finally:
        ctx.cleanup()
    return Result(slug, title, expectation, status, reason, ctx.events, time.time() - start)


def clip(text: str) -> str:
    return text if len(text) <= TRUNCATE else text[:TRUNCATE] + f"\n... {len(text) - TRUNCATE} more characters"


CSS = """
:root{--bg:#fff;--fg:#1b1f24;--muted:#59636e;--line:#d1d9e0;--card:#f6f8fa;--ok:#1a7f37;--bad:#cf222e;--skip:#9a6700;--okbg:#dafbe1;--badbg:#ffebe9}
@media (prefers-color-scheme:dark){:root{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--line:#30363d;--card:#151b23;--ok:#3fb950;--bad:#f85149;--skip:#d29922;--okbg:#12261e;--badbg:#2d1214}}
body{margin:0;padding:24px 16px 64px;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,sans-serif}
main{max-width:960px;margin:0 auto}h1{font-size:24px;margin:0 0 4px}h2{font-size:18px;margin:0;display:inline}
p.meta{color:var(--muted);margin:0 0 20px}table{border-collapse:collapse;width:100%;margin-bottom:28px}
td,th{padding:6px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}
.pass{color:var(--ok);font-weight:600}.fail{color:var(--bad);font-weight:600}.skip{color:var(--skip);font-weight:600}
details{border:1px solid var(--line);border-radius:8px;margin:0 0 16px;background:var(--card)}
details>summary{padding:12px 16px;cursor:pointer}
details.s-pass{border-left:4px solid var(--ok)}details.s-fail{border-left:4px solid var(--bad)}details.s-skip{border-left:4px solid var(--skip)}
.body{padding:0 16px 16px}.why{margin:4px 0 12px;color:var(--muted)}.event{margin:8px 0}
pre{margin:4px 0 0;padding:8px 10px;background:var(--bg);border:1px solid var(--line);border-radius:6px;overflow-x:auto;font:12.5px/1.4 ui-monospace,Menlo,monospace;white-space:pre-wrap}
.cmd{font-weight:600}.tag{font-size:12px;color:var(--muted)}
.check{padding:3px 8px;border-radius:4px}.check.ok{background:var(--okbg)}.check.bad{background:var(--badbg)}
.note{color:var(--muted);font-style:italic}img{max-width:100%;border:1px solid var(--line);border-radius:6px;margin-top:4px}
.fail-reason{color:var(--bad)}code{font:12.5px ui-monospace,Menlo,monospace}
"""


def render_page(results: list[Result], builds: list[Build], title: str) -> str:
    passed = sum(r.status == "PASS" for r in results)
    skipped = sum(r.status == "SKIP" for r in results)
    parts = [f"<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>"
             f"<title>{html.escape(title)}</title><style>{CSS}</style><main>",
             f"<h1>{html.escape(title)}</h1>",
             f"<p class=meta>{passed} of {len(results)} scenarios pass, {skipped} skipped &middot; "
             f"{time.strftime('%Y-%m-%d %H:%M:%S')} &middot; design: <code>doc/comments-design.md</code> section 9.10</p>"]
    for b in builds:
        parts.append(f"<p>Build: <span class={'pass' if b.ok else 'fail'}>{'ok' if b.ok else 'FAILED'}</span>: {html.escape(b.reason)}</p>")
        if b.log and not b.ok:
            parts.append(f"<pre>{html.escape(clip(b.log))}</pre>")
    parts.append("<table><tr><th>#<th>Scenario<th>Result<th>First failure")
    for r in results:
        parts.append(f"<tr><td>{r.slug[:2]}<td><a href='#{r.slug}'>{html.escape(r.title)}</a>"
                     f"<td class={r.status.lower()}>{r.status}<td>{html.escape(r.reason)}")
    parts.append("</table>")
    for r in results:
        parts.append(f"<details id='{r.slug}' class='s-{r.status.lower()}' {'open' if r.status == 'FAIL' else ''}>"
                     f"<summary><h2>{r.slug[:2]}. {html.escape(r.title)}</h2> "
                     f"<span class={r.status.lower()}>{r.status}</span> <span class=tag>{r.seconds:.1f}s</span></summary><div class=body>")
        parts.append(f"<p class=why>Expected: {html.escape(r.expectation)}</p>")
        if r.status != "PASS":
            cls = "fail-reason" if r.status == "FAIL" else "note"
            parts.append(f"<p class={cls}>{'Stopped' if r.status == 'FAIL' else 'Skipped'}: {html.escape(r.reason)}</p>")
        for e in r.events:
            if e.kind == "note":
                parts.append(f"<div class='event note'>{html.escape(e.text)}</div>")
            elif e.kind == "check":
                detail = f": <code>{html.escape(clip(e.detail))}</code>" if e.detail and not e.ok else ""
                parts.append(f"<div class='event check {'ok' if e.ok else 'bad'}'>{'&#10003;' if e.ok else '&#10007;'} "
                             f"{html.escape(e.text)}{detail}</div>")
            elif e.kind == "run":
                parts.append(f"<div class=event><span class=tag>{html.escape(e.where)} &middot; "
                             f"{'exit ' + str(e.code) if e.code >= 0 else 'not run'} &middot; {html.escape(e.detail)}</span>"
                             f"<pre class=cmd>$ {html.escape(e.text)}</pre>")
                if e.err.strip() and not e.ok:
                    parts.append(f"<pre>{html.escape(clip(e.err))}</pre>")
                parts.append("</div>")
            elif e.kind == "metrics":
                parts.append(f"<details class=event><summary class=tag>comments metrics: {html.escape(e.text)}</summary>"
                             f"<pre>{html.escape(clip(e.detail))}</pre></details>")
            elif e.kind == "shot":
                parts.append(f"<div class=event><span class=tag>screenshot: {html.escape(e.text)}</span><br>"
                             f"<a href='{html.escape(e.file)}'><img src='{html.escape(e.file)}' alt='{html.escape(e.text)}' loading=lazy></a></div>")
        parts.append("</div></details>")
    parts.append("</main>")
    return "".join(parts)


def select(only: str | None) -> list[tuple[str, str, str, object, bool]]:
    chosen = []
    for entry in SCENARIOS:
        if only and only not in entry[0]:
            continue
        if CFG.skip_real and entry[4]:
            continue
        chosen.append(entry)
    return chosen


def run_all(only: str | None, builds: list[Build], title: str, quiet: bool = False) -> list[Result]:
    shutil.rmtree(CFG.out, ignore_errors=True)
    CFG.out.mkdir(parents=True)
    results = []
    for slug, name, expectation, fn, _ in select(only):
        result = run_scenario(slug, name, expectation, fn)
        results.append(result)
        if not quiet:
            print(f"  {slug[:2]} {name:<58} {result.status}" + ("" if result.status == "PASS" else f"  {result.reason[:110]}"),
                  flush=True)
    (CFG.out / "index.html").write_text(render_page(results, builds, title), encoding="utf-8")
    return results


STUB_MARQ = r'''#!/usr/bin/env python3
import json, os, subprocess, sys, time

args = sys.argv[1:]
o = {"width": 960.0, "comments": "show", "numbers": "off", "click": None, "print": False, "settle": None,
     "metrics": None, "png": None, "pdf": None, "file": None}
i = 0
while i < len(args):
    a = args[i]
    if a == "--width": o["width"] = float(args[i + 1]); i += 1
    elif a == "--dump-metrics": o["metrics"] = args[i + 1]; i += 1
    elif a == "--export-png": o["png"] = args[i + 1]; i += 1
    elif a == "--export-pdf": o["pdf"] = args[i + 1]; i += 1
    elif a == "--comments": o["comments"] = args[i + 1]; i += 1
    elif a == "--comment-numbers": o["numbers"] = args[i + 1]; i += 1
    elif a == "--comments-click": o["click"] = args[i + 1]; i += 1
    elif a == "--settle": o["settle"] = float(args[i + 1]); i += 1
    elif a == "--timeout": i += 1
    elif a == "--print": o["print"] = True
    elif a == "--harness-run": pass
    elif not a.startswith("-") and o["file"] is None: o["file"] = a
    i += 1

BAD = os.environ.get("STUB_BAD", "")
path = o["file"]
text = open(path, encoding="utf-8").read()
lines = text.split("\n")
nonblank = [n for n, l in enumerate(lines, 1) if l.strip()]
base = {"mode": "print" if o["print"] else "screen", "viewportWidth": o["width"], "documentHeight": 24 * len(lines),
        "headings": sum(1 for l in lines if l.startswith("#")), "tableCount": 0,
        "problems": {"brokenWords": [], "overflowing": [], "atMinScale": [], "missingImages": []},
        "tables": [], "images": []}


def load_threads():
    binary = os.environ.get("MARQ_COMMENTS_BIN", "")
    if not binary or not os.path.exists(binary):
        return None
    directory = os.path.dirname(os.path.abspath(path))
    try:
        proc = subprocess.run([binary, "-C", directory, "list", os.path.basename(path), "--json"],
                              capture_output=True, text=True, timeout=5, cwd=directory)
    except Exception:
        return None
    if proc.returncode != 0:
        return None
    try:
        return json.loads(proc.stdout)
    except Exception:
        return None


def block_for(threads):
    shown = o["comments"] != "hide"
    if BAD == "hidden-leak":
        shown_marks = True
    else:
        shown_marks = shown
    offsets = [0]
    for line in lines:
        offsets.append(offsets[-1] + len(line) + 1)
    rail = o["width"] >= 1009
    entries = []
    for index, t in enumerate(threads):
        anchor = t["anchor"]
        marks, text_parts = [], []
        if anchor["status"] in ("anchored", "changed") and shown_marks:
            for n, line in enumerate(lines, 1):
                lo, hi = max(anchor["start"], offsets[n - 1]), min(anchor["end"], offsets[n - 1] + len(line))
                piece = text[lo:hi].strip() if hi > lo else ""
                if piece:
                    top = 24 * n + (2 if (BAD == "numbers-move" and o["numbers"] == "on") else 0)
                    marks.append({"top": top, "left": 10, "width": 8 * len(piece), "height": 20})
                    text_parts.append(piece)
        entries.append({"t": t, "marks": marks, "text": " ".join(text_parts)})
    entries.sort(key=lambda e: (e["marks"][0]["top"] if e["marks"] else 10 ** 9))
    out, orphans, bottom, count = [], [], 0, 0
    for number, e in enumerate(entries, 1):
        t, anchor = e["t"], e["t"]["anchor"]
        card = None
        if shown and anchor["status"] in ("anchored", "changed"):
            top = e["marks"][0]["top"] if e["marks"] else bottom
            if not rail and e["marks"]:
                top += e["marks"][0]["height"] + 4
            if BAD != "overlap":
                top = max(top, bottom + 8) if bottom else top
            if BAD == "rail-offset" and number == 1:
                top += 10
            card = {"top": top, "left": 1100, "width": 280, "height": 50, "inRail": rail}
            bottom = top + 50
        if shown and anchor["status"] in ("orphaned", "applied"):
            orphans.append({"id": t["annotation"]["id"], "status": anchor["status"]})
        marked = e["text"] + ("x" if BAD == "marked-text" and e["text"] else "")
        out.append({"id": t["annotation"]["id"], "number": number, "state": t["state"], "status": anchor["status"],
                    "anchorText": anchor.get("text"), "markedText": marked, "exact": marked == anchor.get("text"),
                    "flag": None, "marks": e["marks"], "card": card, "offsetPx": 0, "replies": len(t["replies"])})
        count += 1 if e["marks"] else 0
    overlaps = []
    cards = [x for x in out if x["card"]]
    for a in range(len(cards)):
        for b in range(a + 1, len(cards)):
            ca, cb = cards[a]["card"], cards[b]["card"]
            if ca["top"] < cb["top"] + cb["height"] and cb["top"] < ca["top"] + ca["height"]:
                overlaps.append([cards[a]["id"], cards[b]["id"]])
    active = None
    if o["click"] and BAD != "no-active":
        active = o["click"].split(":", 1)[1]
    gutter = [{"line": n, "top": 24 * n + (1 if (BAD == "gutter-shift" and shown) else 0)} for n in nonblank]
    return {"shown": shown, "numbers": o["numbers"] == "on", "layout": ("rail" if rail else "stacked") if shown and cards else "none",
            "mapper": "ok", "threadCount": len(out), "markedCount": count, "unmarkedCount": len(out) - count,
            "orphanCount": len(orphans), "active": active, "threads": out, "overlaps": overlaps,
            "orphans": orphans, "gutter": gutter}


if o["settle"]:
    time.sleep(o["settle"])
threads = load_threads()
metrics = dict(base)
if threads is not None:
    metrics["comments"] = block_for(threads)
if o["pdf"]:
    with open(o["pdf"], "w") as f:
        f.write("pages=%d" % (1 + len(text) // 2000))
elif o["png"]:
    import base64
    with open(o["png"], "wb") as f:
        f.write(base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="))
elif o["metrics"] == "-":
    print(json.dumps(metrics))
elif o["metrics"]:
    with open(o["metrics"], "w") as f:
        json.dump(metrics, f)
'''

STUB_CLI = r'''#!/usr/bin/env python3
import difflib, json, os, sys, uuid

args = sys.argv[1:]
agent, author, directory = False, None, os.getcwd()
rest = []
i = 0
while i < len(args):
    if args[i] == "--agent": agent = True
    elif args[i] == "--author": author = args[i + 1]; i += 1
    elif args[i] == "-C": directory = args[i + 1]; i += 1
    else: rest.append(args[i])
    i += 1
command, rest = rest[0], rest[1:]
opts, positional = {}, []
i = 0
while i < len(rest):
    a = rest[i]
    if a in ("--line", "--text", "--nth", "--range", "--replace", "-m", "--message", "--state"):
        opts[a] = rest[i + 1]; i += 1
    elif a == "--json": opts[a] = True
    else: positional.append(a)
    i += 1

root = os.path.abspath(directory)
while not os.path.isdir(os.path.join(root, ".git")):
    parent = os.path.dirname(root)
    if parent == root:
        sys.stderr.write("not a git repository\n")
        sys.exit(1)
    root = parent
state_path = os.path.join(root, ".stubstate.json")
state = json.load(open(state_path)) if os.path.exists(state_path) else []


def save():
    json.dump(state, open(state_path, "w"))


def file_text(name):
    return open(os.path.join(os.path.abspath(directory), name), encoding="utf-8").read()


def find(prefix):
    for t in state:
        if t["id"].startswith(prefix) or any(r["id"].startswith(prefix) for r in t["replies"]):
            return t
    sys.stderr.write("no such thread\n")
    sys.exit(1)


def create(kind):
    name = positional[0]
    text = file_text(name)
    if "--range" in opts:
        start, end = (int(x) for x in opts["--range"].split(":"))
    else:
        line = int(opts["--line"])
        offsets = [0]
        for l in text.split("\n"):
            offsets.append(offsets[-1] + len(l) + 1)
        start, end = offsets[line - 1], offsets[line] - 1
        if "--text" in opts:
            nth = int(opts.get("--nth", "1"))
            row = text[start:end]
            at = -1
            for _ in range(nth):
                at = row.find(opts["--text"], at + 1)
            start, end = start + at, start + at + len(opts["--text"])
    tid = uuid.uuid4().hex
    state.append({"id": tid, "kind": kind, "file": name, "start": start, "end": end, "quote": text[start:end],
                  "replace": opts.get("--replace"), "state": "open", "applied": False, "replies": [],
                  "creator": {"type": "Software" if agent else "Person", "name": author or "Jim"}})
    save()
    print(tid[:8])


def anchor_of(t, text):
    if t["state"] == "accepted":
        if t["replace"] == "":
            return {"status": "applied"}
        quote = t["replace"]
    else:
        quote = t["quote"]
    best = None
    at = text.find(quote)
    while at >= 0 and quote:
        if best is None or abs(at - t["start"]) < abs(best - t["start"]):
            best = at
        at = text.find(quote, at + 1)
    if best is not None:
        return place(text, best, best + len(quote), "anchored", quote)
    words = text.split()
    close = difflib.get_close_matches(quote, words, n=1, cutoff=0.8) if " " not in quote else []
    if close:
        at = text.find(close[0])
        result = place(text, at, at + len(close[0]), "changed", close[0])
        result["original"] = quote
        return result
    return {"status": "orphaned"}


def place(text, start, end, status, quoted):
    line = text.count("\n", 0, start) + 1
    return {"status": status, "start": start, "end": end, "line": line,
            "column": start - (text.rfind("\n", 0, start) + 1) + 1, "text": text[start:end]}


def thread_json(t, text):
    anchor = anchor_of(t, text)
    return {"annotation": {"id": "urn:uuid:" + t["id"], "creator": t["creator"], "motivation": "commenting"},
            "state": t["state"], "anchor": anchor, "stateChanges": [],
            "replies": [{"annotation": {"id": "urn:uuid:" + r["id"], "creator": r["creator"], "motivation": "replying"},
                         "state": t["state"], "anchor": anchor, "stateChanges": [], "replies": []} for r in t["replies"]]}


if command in ("comment", "suggest"):
    create("comment" if command == "comment" else "suggestion")
elif command == "reply":
    t = find(positional[0])
    rid = uuid.uuid4().hex
    t["replies"].append({"id": rid, "creator": {"type": "Software" if agent else "Person", "name": author or "Jim"}})
    save()
    print(rid[:8])
elif command == "accept":
    t = find(positional[0])
    name = t["file"]
    path = os.path.join(root, name)
    text = open(path, encoding="utf-8").read()
    at = text.find(t["quote"])
    open(path, "w", encoding="utf-8").write(text[:at] + t["replace"] + text[at + len(t["quote"]):])
    t["state"] = "accepted"
    save()
elif command == "list":
    text = file_text(positional[0])
    print(json.dumps([thread_json(t, text) for t in state]))
else:
    sys.stderr.write("unsupported in the stub: %s\n" % command)
    sys.exit(2)
'''

STUB_PDFTOOL = r'''#!/usr/bin/env python3
import json, re, sys

text = open(sys.argv[2]).read()
pages = int(re.search(r"pages=(\d+)", text).group(1))
print(json.dumps({"pageCount": pages, "pages": []}))
'''

BAD_MODES = [
    ("marked-text", "01-one-word"),
    ("rail-offset", "01-one-word"),
    ("overlap", "10-close-threads"),
    ("numbers-move", "16-numbers"),
    ("hidden-leak", "17-comments-hidden"),
    ("gutter-shift", "21-gutter"),
    ("no-active", "20-click"),
]


def write_stub(directory: Path, name: str, source: str) -> Path:
    path = directory / name
    path.write_text(source)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return path


def self_test() -> int:
    work = Path(tempfile.mkdtemp(prefix="marq-comments-selftest-"))
    CFG.marq = write_stub(work, "marq", STUB_MARQ)
    CFG.cli = write_stub(work, "marq-comments", STUB_CLI)
    CFG.pdftool = write_stub(work, "pdftool", STUB_PDFTOOL)
    CFG.out = work / "page"
    CFG.skip_real = True
    CFG.new_delay, CFG.new_settle, CFG.hang_limit = 1.5, 4.0, 15.0
    os.environ.pop("STUB_BAD", None)
    print("self-test: stub marq, stub marq-comments and stub pdftool in", work)
    print("good canned metrics: every scenario must pass")
    builds = [Build(True, "self-test: nothing is built")]
    results = run_all(None, builds, "comments acceptance self-test")
    failures = []
    for r in results:
        if r.status != "PASS":
            failures.append(f"{r.slug} should pass on good metrics but {r.status}: {r.reason}")
    print("bad canned metrics: the named scenario must fail")
    for mode, slug in BAD_MODES:
        os.environ["STUB_BAD"] = mode
        CFG.out = work / f"page-{mode}"
        outcome = run_all(slug, builds, f"self-test {mode}", quiet=True)
        status = outcome[0].status if outcome else "MISSING"
        reason = outcome[0].reason if outcome else ""
        print(f"  {mode:<14} {slug:<22} {status}" + (f"  {reason[:90]}" if status == "FAIL" else ""))
        if status != "FAIL":
            failures.append(f"{slug} should fail with STUB_BAD={mode} but {status}")
    os.environ.pop("STUB_BAD", None)
    shutil.rmtree(work, ignore_errors=True)
    if failures:
        print("\nself-test FAILED")
        for f in failures:
            print("  " + f)
        return 1
    print(f"\nself-test passed: {len(results)} scenarios pass on good metrics, {len(BAD_MODES)} bad modes fail where expected.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Acceptance run for comments in the marq window (design 9.10).")
    parser.add_argument("--open", action="store_true", help="open the page when done")
    parser.add_argument("--no-build", action="store_true", help="do not run swift build and cargo build first")
    parser.add_argument("--no-bundle", action="store_true", help="skip the bundled-app scenario, which runs just bundle")
    parser.add_argument("--only", metavar="SLUG", help="run only scenarios whose slug contains SLUG")
    parser.add_argument("--self-test", action="store_true", help="run the harness against stub binaries, building nothing")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    for tool in ("git", "python3"):
        if shutil.which(tool) is None:
            print(f"{tool} is not installed", file=sys.stderr)
            return 2
    if not EXAMPLES.exists():
        print(f"missing {EXAMPLES}", file=sys.stderr)
        return 2

    CFG.no_bundle = args.no_bundle
    print("comments acceptance")
    builds = build_all(args.no_build)
    for b in builds:
        print(f"  build: {'ok' if b.ok else 'FAILED'}: {b.reason}")
    results = run_all(args.only, builds, "Comments in marq: acceptance")
    page = CFG.out / "index.html"
    passed = sum(r.status == "PASS" for r in results)
    skipped = sum(r.status == "SKIP" for r in results)
    print(f"\n{passed} of {len(results)} scenarios pass, {skipped} skipped.\nPage: {page}")
    if args.open:
        opener = "open" if platform.system() == "Darwin" else "xdg-open"
        if shutil.which(opener):
            subprocess.run([opener, str(page)], check=False)
    return 0 if all(r.status in ("PASS", "SKIP") for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
