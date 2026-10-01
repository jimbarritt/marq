#!/usr/bin/env python3
"""Acceptance run for marq-comments.

Copies example-docs/test.md into temporary git repositories, drives the real
`marq-comments` binary through the scenarios of doc/comments-design.md section
7.1, checks what it printed, and writes one page to read:

    cli/target/acceptance/index.html

The page has a section per scenario: the commands that ran, their output, each
check with its verdict, and the page `marq-comments render` produced. Run it
with `cd cli && just acceptance`.

The run is written before the CLI exists, so it fails now and measures progress
as the CLI is built. A scenario that cannot run says why. Nothing here touches
example-docs/ or this repository's own md-comments branch: every scenario works
on a copy in a temporary directory.

Standard library only, Python 3.9 or later.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

CLI = Path(__file__).resolve().parents[2]
ROOT = CLI.parent
EXAMPLE = ROOT / "example-docs" / "test.md"
OUT = CLI / "target" / "acceptance"
# MARQ_COMMENTS_BIN points the run at another binary, for example an installed
# one, and skips the build. It also lets the harness itself be tested against a stub.
BINARY = Path(os.environ.get("MARQ_COMMENTS_BIN") or CLI / "target" / "debug" / "marq-comments")
DOC = "docs/test.md"

# Tests must not depend on the machine's git configuration. This container's
# global `push.negotiate true` printed errors on local pushes, which is how
# that came to be known.
GIT_ENV = {
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_TERMINAL_PROMPT": "0",
}

ID_PATTERN = re.compile(r"[0-9a-f]{8}")
TRUNCATE = 6000


class ScenarioFailed(Exception):
    """The first failed check stops a scenario: later steps depend on it."""


# --- the build ---------------------------------------------------------------


@dataclass
class Build:
    ok: bool
    reason: str
    log: str = ""


def build(skip: bool) -> Build:
    if os.environ.get("MARQ_COMMENTS_BIN"):
        found = BINARY.exists()
        return Build(found, f"using MARQ_COMMENTS_BIN={BINARY}" if found else f"MARQ_COMMENTS_BIN={BINARY} does not exist")
    if not (CLI / "Cargo.toml").exists():
        return Build(False, "cli/Cargo.toml does not exist: the crate is not scaffolded yet (task T-04)")
    if skip:
        return Build(BINARY.exists(), "skipped by --no-build" if BINARY.exists() else "--no-build and no binary at target/debug/marq-comments")
    try:
        proc = subprocess.run(["cargo", "build"], cwd=CLI, capture_output=True, text=True, timeout=900)
    except FileNotFoundError:
        return Build(False, "cargo is not installed")
    except subprocess.TimeoutExpired:
        return Build(False, "cargo build took over 15 minutes")
    log = (proc.stdout + proc.stderr)[-TRUNCATE:]
    if proc.returncode != 0:
        return Build(False, f"cargo build failed (exit {proc.returncode})", log)
    if not BINARY.exists():
        return Build(False, "cargo build succeeded but target/debug/marq-comments does not exist: is the binary named marq-comments?", log)
    return Build(True, "built", log)


# --- what a scenario records -------------------------------------------------


@dataclass
class Event:
    kind: str  # run, check, note, render
    text: str = ""
    ok: bool = True
    detail: str = ""
    code: int = 0
    out: str = ""
    err: str = ""
    where: str = ""
    file: str = ""


class Ctx:
    def __init__(self, slug: str, built: Build):
        self.slug = slug
        self.built = built
        self.events: list[Event] = []
        self.tmp = Path(tempfile.mkdtemp(prefix=f"marq-acceptance-{slug}-"))
        self.renders = 0

    def note(self, text: str) -> None:
        self.events.append(Event("note", text))

    def check(self, description: str, condition: bool, detail: str = "") -> None:
        self.events.append(Event("check", description, bool(condition), detail))
        if not condition:
            raise ScenarioFailed(description + (f" ({detail})" if detail else ""))

    def cleanup(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)


class Repo:
    """A temporary git repository holding docs/test.md."""

    def __init__(self, ctx: Ctx, name: str, path: Path):
        self.ctx = ctx
        self.name = name
        self.path = path

    @staticmethod
    def create(ctx: Ctx, name: str, remote: Path | None = None) -> "Repo":
        path = ctx.tmp / name
        path.mkdir(parents=True)
        repo = Repo(ctx, name, path)
        repo.git("init", "-q", "-b", "main")
        repo.configure()
        (path / "docs").mkdir()
        shutil.copyfile(EXAMPLE, path / DOC)
        repo.git("add", DOC)
        repo.git("commit", "-q", "-m", "Add test document")
        if remote is not None:
            repo.git("remote", "add", "origin", str(remote))
            repo.git("push", "-q", "origin", "main")
        return repo

    @staticmethod
    def clone(ctx: Ctx, name: str, remote: Path) -> "Repo":
        path = ctx.tmp / name
        subprocess.run(["git", "clone", "-q", str(remote), str(path)], check=True,
                       capture_output=True, env={**os.environ, **GIT_ENV})
        repo = Repo(ctx, name, path)
        repo.configure()
        return repo

    def configure(self, user: str = "Jim", email: str = "jim@example.com") -> None:
        self.git("config", "user.name", user)
        self.git("config", "user.email", email)

    def git(self, *args: str) -> str:
        proc = subprocess.run(["git", *args], cwd=self.path, capture_output=True, text=True,
                              env={**os.environ, **GIT_ENV})
        if proc.returncode != 0:
            raise RuntimeError(f"git {' '.join(args)} failed in {self.name}: {proc.stderr.strip()}")
        return proc.stdout

    # -- the markdown ----------------------------------------------------------

    def read(self) -> str:
        return (self.path / DOC).read_text(encoding="utf-8")

    def write(self, text: str) -> None:
        (self.path / DOC).write_text(text, encoding="utf-8")

    def edit(self, old: str, new: str) -> None:
        text = self.read()
        self.ctx.check(f"setup: {old[:40]!r} is in the markdown exactly once", text.count(old) == 1,
                       f"found {text.count(old)}")
        self.write(text.replace(old, new))
        self.ctx.note(f"{self.name}: edited the markdown, {old[:50]!r} became {new[:50]!r}")

    def find(self, word: str) -> tuple[int, int]:
        """The 1-based (line, column) of the first occurrence, in code points."""
        text = self.read()
        index = text.find(word)
        self.ctx.check(f"setup: {word!r} is in the markdown", index >= 0)
        line = text.count("\n", 0, index) + 1
        column = index - (text.rfind("\n", 0, index) + 1) + 1
        return line, column

    def line_text(self, line: int) -> str:
        return self.read().split("\n")[line - 1]

    # -- the CLI ---------------------------------------------------------------

    def cli(self, *args: str, expect: int = 0) -> str:
        shown = "marq-comments " + " ".join(a if re.fullmatch(r"[\w./:=@-]+", a) else repr(a) for a in args)
        if not self.ctx.built.ok:
            self.ctx.events.append(Event("run", shown, False, self.ctx.built.reason, -1, where=self.name))
            raise ScenarioFailed("marq-comments is not built (see Build above)")
        start = time.time()
        try:
            proc = subprocess.run([str(BINARY), *args], cwd=self.path, capture_output=True, text=True,
                                  timeout=60, env={**os.environ, **GIT_ENV})
        except subprocess.TimeoutExpired:
            self.ctx.events.append(Event("run", shown, False, "timed out after 60 seconds", -1, where=self.name))
            raise ScenarioFailed("command timed out: " + shown)
        ok = proc.returncode == expect
        detail = f"{time.time() - start:.2f}s" + ("" if ok else f", expected exit {expect}")
        self.ctx.events.append(Event("run", shown, ok, detail, proc.returncode, proc.stdout, proc.stderr, self.name))
        if not ok:
            raise ScenarioFailed(f"exit code {proc.returncode}, expected {expect}: {shown}")
        return proc.stdout

    def threads(self, *extra: str) -> list[dict]:
        out = self.cli("list", DOC, "--json", *extra)
        try:
            data = json.loads(out)
        except json.JSONDecodeError as error:
            self.ctx.check("list --json prints valid JSON", False, str(error))
        self.ctx.check("list --json prints an array of threads", isinstance(data, list), type(data).__name__)
        return data

    def thread(self, short_id: str, threads: list[dict] | None = None) -> dict:
        for t in threads if threads is not None else self.threads():
            if t["annotation"]["id"].replace("urn:uuid:", "").startswith(short_id):
                return t
        self.ctx.check(f"list --json includes thread {short_id}", False)
        raise AssertionError  # unreachable: check raises

    def render(self, label: str) -> None:
        self.ctx.renders += 1
        name = f"{self.ctx.slug}-{self.ctx.renders}-{label}.html"
        target = OUT / "scenarios" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        self.cli("render", DOC, "-o", str(target))
        self.ctx.check(f"render wrote {name}", target.exists() and target.stat().st_size > 0)
        self.ctx.events.append(Event("render", label, True, file=f"scenarios/{name}", where=self.name))


def new_id(ctx: Ctx, out: str, what: str) -> str:
    short = out.strip()
    ctx.check(f"{what} prints an 8-character id on one line", ID_PATTERN.fullmatch(short) is not None, repr(short))
    return short


def states(t: dict) -> list[str]:
    return [change["marq:state"] for change in t["stateChanges"]]


# --- the scenarios -----------------------------------------------------------

SCENARIOS: list[tuple[str, str, str, object]] = []


def scenario(slug: str, title: str, expectation: str):
    def register(fn):
        SCENARIOS.append((slug, title, expectation, fn))
        return fn
    return register


@scenario("01-word-comment", "A comment on a word, then an agent reply",
          "The word is marked on the page, with a thread of one Person comment and one Software reply. "
          "The comment lives on md-comments and the working tree stays clean.")
def word_comment(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, column = r.find("reloading")
    cid = new_id(c, r.cli("comment", DOC, "--line", str(line), "--text", "reloading",
                          "-m", "Is reloading the right word here?"), "comment")
    rid = new_id(c, r.cli("--agent", "--author", "Claude <noreply@anthropic.com>", "reply", cid,
                          "-m", "Yes. The page does not reload."), "reply")
    t = r.thread(cid)
    c.check("the thread is open", t["state"] == "open", t["state"])
    c.check("the anchor is anchored on the word", t["anchor"]["status"] == "anchored" and t["anchor"]["text"] == "reloading",
            json.dumps(t["anchor"]))
    c.check("the anchor has the word's line and column", (t["anchor"]["line"], t["anchor"]["column"]) == (line, column),
            f'got {t["anchor"].get("line")}:{t["anchor"].get("column")}, want {line}:{column}')
    c.check("the comment's creator is a Person named Jim",
            t["annotation"]["creator"]["type"] == "Person" and t["annotation"]["creator"]["name"] == "Jim")
    c.check("the thread has one reply", len(t["replies"]) == 1, str(len(t["replies"])))
    reply = t["replies"][0]["annotation"]
    c.check("the reply targets the comment", reply["target"] == t["annotation"]["id"], str(reply.get("target")))
    c.check("the reply's motivation is replying", reply["motivation"] == "replying")
    c.check("the reply's creator is Software", reply["creator"]["type"] == "Software", json.dumps(reply["creator"]))
    c.check("the reply id matches the printed id", reply["id"].replace("urn:uuid:", "").startswith(rid))
    branch = r.git("ls-tree", "-r", "--name-only", "refs/heads/md-comments")
    c.check("the annotation file is under documents/docs/test.md/annotations/",
            f"documents/docs/test.md/annotations/{t['annotation']['id'].replace('urn:uuid:', '')}.json" in branch.split("\n"))
    c.check("the working tree is clean", r.git("status", "--porcelain").strip() == "", r.git("status", "--porcelain"))
    r.render("thread")


@scenario("02-line-comment", "A comment on a whole line",
          "The whole line is marked, not a word.")
def line_comment(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, _ = r.find("This is a test document")
    text = r.line_text(line)
    cid = new_id(c, r.cli("comment", DOC, "--line", str(line), "-m", "Is this the right opening line?"), "comment")
    t = r.thread(cid)
    c.check("the anchor is anchored", t["anchor"]["status"] == "anchored", json.dumps(t["anchor"]))
    c.check("the anchor text is the whole line", t["anchor"]["text"] == text, repr(t["anchor"].get("text")))
    c.check("the anchor starts in column 1 of that line", (t["anchor"]["line"], t["anchor"]["column"]) == (line, 1))
    selectors = t["annotation"]["target"]["selector"]
    c.check("the stored target has a line fragment selector",
            any(s.get("type") == "FragmentSelector" and s.get("value") == f"line={line - 1},{line}" for s in selectors),
            json.dumps(selectors))
    r.render("line")


@scenario("03-suggestion-accepted", "A suggestion, accepted",
          "Accepting edits the markdown on the working branch and changes the state to accepted. "
          "The accepted suggestion is anchored on the text it put in the file. "
          "Nothing is committed to the working branch. An accepted suggestion cannot reopen.")
def suggestion_accepted(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, _ = r.find("awkward")
    sid = new_id(c, r.cli("suggest", DOC, "--line", str(line), "--text", "awkward", "--replace", "peculiar",
                          "-m", "A better word."), "suggestion")
    t = r.thread(sid)
    c.check("the motivation is editing", t["annotation"]["motivation"] == "editing", t["annotation"]["motivation"])
    c.check("the suggestion is open and anchored", t["state"] == "open" and t["anchor"]["status"] == "anchored")
    r.render("before-accept")
    r.cli("accept", sid)
    c.check("the markdown now says peculiar", "Note the peculiar slugs" in r.read())
    c.check("the markdown no longer says awkward in that place", "Note the awkward slugs" not in r.read())
    c.check("the state is accepted", r.thread(sid)["state"] == "accepted")
    after = r.thread(sid)["anchor"]
    c.check("the accepted suggestion is anchored on the replacement, not changed",
            after["status"] == "anchored" and after["text"] == "peculiar", json.dumps(after))
    c.check("the anchor sits at the replacement's line and column",
            (after["line"], after["column"]) == r.find("peculiar"), f'{after.get("line")}:{after.get("column")}')
    listed = r.cli("list", DOC)
    c.check("the text output shows the replacement and no changed flag", '"peculiar"' in listed and "changed" not in listed, listed[:200])
    c.check("the edit is in the working tree, uncommitted", r.git("status", "--porcelain").strip() == f"M {DOC}",
            r.git("status", "--porcelain"))
    c.check("no commit was made on main", r.git("log", "--format=%s").strip() == "Add test document")
    r.cli("reopen", sid, expect=1)
    c.check("a failed reopen leaves the state accepted", r.thread(sid)["state"] == "accepted")
    r.render("after-accept")


@scenario("04-suggestion-rejected", "A suggestion, rejected, then reopened",
          "Rejecting leaves the markdown alone. A rejected suggestion reopens, and both state changes stay in the history.")
def suggestion_rejected(c: Ctx) -> None:
    r = Repo.create(c, "work")
    before = r.read()
    line, _ = r.find("awkward")
    sid = new_id(c, r.cli("suggest", DOC, "--line", str(line), "--text", "awkward", "--replace", "peculiar"), "suggestion")
    r.cli("reject", sid)
    t = r.thread(sid)
    c.check("the state is rejected", t["state"] == "rejected", t["state"])
    c.check("the markdown is unchanged", r.read() == before)
    r.cli("reopen", sid)
    t = r.thread(sid)
    c.check("the state is open again", t["state"] == "open", t["state"])
    c.check("the history lists rejected then open", states(t) == ["rejected", "open"], str(states(t)))
    r.render("reopened")


@scenario("05-paragraph-added-above", "A paragraph added above the comments",
          "Every anchor stays anchored, at its shifted position.")
def paragraph_added(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, _ = r.find("reloading")
    cid = new_id(c, r.cli("comment", DOC, "--line", str(line), "--text", "reloading", "-m", "Check this word."), "comment")
    start = r.thread(cid)["anchor"]["start"]
    opening = "This is a test document for **Marq**, a macOS markdown viewer.\n"
    added = "\nA new paragraph, added above every comment.\n"
    r.edit(opening, opening + added)
    t = r.thread(cid)
    c.check("the anchor is anchored", t["anchor"]["status"] == "anchored", json.dumps(t["anchor"]))
    c.check("the anchor text is still the word", t["anchor"]["text"] == "reloading")
    c.check("the anchor moved by the length of the added text", t["anchor"]["start"] == start + len(added),
            f'start {t["anchor"].get("start")}, want {start + len(added)}')
    c.check("the line moved down by the added lines", t["anchor"]["line"] == line + added.count("\n"),
            f'line {t["anchor"].get("line")}, want {line + added.count(chr(10))}')
    r.render("shifted")


@scenario("06-paragraph-moved", "A commented paragraph moved to the end of the file",
          "The anchor follows the paragraph and is anchored at the new position.")
def paragraph_moved(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, _ = r.find("reloading")
    cid = new_id(c, r.cli("comment", DOC, "--line", str(line), "--text", "reloading", "-m", "Check this word."), "comment")
    text = r.read()
    first = text.index("Every entry below")
    end_marker = "produces a double hyphen.\n"
    last = text.index(end_marker) + len(end_marker)
    paragraph = text[first:last]
    r.write(text[:first] + text[last:].lstrip("\n") + "\n" + paragraph)
    c.note("work: the paragraph from 'Every entry below' to 'double hyphen.' moved to the end of the file")
    t = r.thread(cid)
    c.check("the anchor is anchored", t["anchor"]["status"] == "anchored", json.dumps(t["anchor"]))
    c.check("the anchor is at the word's new position", t["anchor"]["start"] == r.read().index("reloading"),
            f'start {t["anchor"].get("start")}, want {r.read().index("reloading")}')
    r.render("moved")


@scenario("07-typo-fixed", "A typo fixed in a commented word",
          "The anchor is marked changed, shows the original quote, and points at the corrected word.")
def typo_fixed(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, _ = r.find("reloading")
    cid = new_id(c, r.cli("comment", DOC, "--line", str(line), "--text", "reloading", "-m", "Check this word."), "comment")
    r.edit("reloading", "reloadng")
    t = r.thread(cid)
    c.check("the anchor is changed", t["anchor"]["status"] == "changed", json.dumps(t["anchor"]))
    c.check("the anchor keeps the original quote", t["anchor"].get("original") == "reloading", str(t["anchor"].get("original")))
    c.check("the anchor points at the new text", t["anchor"].get("text") == "reloadng", str(t["anchor"].get("text")))
    out = r.cli("list", DOC)
    c.check("the text output says changed and shows the original", "changed" in out and "reloading" in out)
    r.render("changed")


@scenario("08-word-deleted", "A commented word deleted",
          "The anchor is orphaned and listed below the text. It does not attach to the same word elsewhere in the file.")
def word_deleted(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, _ = r.find("awkward")
    c.check("setup: the word occurs more than once in the file", r.read().count("awkward") > 1, str(r.read().count("awkward")))
    cid = new_id(c, r.cli("comment", DOC, "--line", str(line), "--text", "awkward", "-m", "Check this word."), "comment")
    r.edit("Note the awkward slugs:", "Note the slugs:")
    t = r.thread(cid)
    c.check("the anchor is orphaned", t["anchor"]["status"] == "orphaned", json.dumps(t["anchor"]))
    out = r.cli("list", DOC)
    c.check("the text output says orphaned", "orphaned" in out, out[:200])
    r.render("orphaned")


@scenario("09-comment-resolved", "A comment resolved",
          "Resolving changes the state without deleting anything. The history keeps every state.")
def comment_resolved(c: Ctx) -> None:
    r = Repo.create(c, "work")
    line, _ = r.find("reloading")
    cid = new_id(c, r.cli("comment", DOC, "--line", str(line), "--text", "reloading", "-m", "Check this word."), "comment")
    r.cli("resolve", cid)
    t = r.thread(cid)
    c.check("the state is resolved", t["state"] == "resolved", t["state"])
    c.check("list --state open leaves it out", all(x["annotation"]["id"] != t["annotation"]["id"] for x in r.threads("--state", "open")))
    c.check("list --state resolved includes it", any(x["annotation"]["id"] == t["annotation"]["id"] for x in r.threads("--state", "resolved")))
    r.render("resolved")
    r.cli("reopen", cid)
    t = r.thread(cid)
    c.check("reopening makes it open", t["state"] == "open", t["state"])
    c.check("the history lists resolved then open", states(t) == ["resolved", "open"], str(states(t)))
    c.check("the annotation file is still on md-comments",
            f"documents/docs/test.md/annotations/{t['annotation']['id'].replace('urn:uuid:', '')}.json"
            in r.git("ls-tree", "-r", "--name-only", "refs/heads/md-comments").split("\n"))


@scenario("10-two-clones-sync", "A second clone adds a comment, and both sync",
          "Both clones list both comments, and the shared remote holds the md-comments branch.")
def two_clones(c: Ctx) -> None:
    remote = c.tmp / "remote.git"
    subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(remote)], check=True,
                   capture_output=True, env={**os.environ, **GIT_ENV})
    a = Repo.create(c, "clone-a", remote)
    b = Repo.clone(c, "clone-b", remote)
    b.configure("Ana", "ana@example.com")
    line, _ = a.find("reloading")
    first = new_id(c, a.cli("comment", DOC, "--line", str(line), "--text", "reloading", "-m", "From clone A."), "comment in A")
    a.cli("sync")
    line, _ = b.find("awkward")
    second = new_id(c, b.cli("comment", DOC, "--line", str(line), "--text", "awkward", "-m", "From clone B."), "comment in B")
    b.cli("sync")
    a.cli("sync")
    for repo in (a, b):
        ids = sorted(t["annotation"]["id"].replace("urn:uuid:", "")[:8] for t in repo.threads())
        c.check(f"{repo.name} lists both comments", ids == sorted([first, second]), f"{ids} vs {sorted([first, second])}")
    remote_branch = subprocess.run(["git", "--git-dir", str(remote), "rev-parse", "--verify", "refs/heads/md-comments"],
                                   capture_output=True, text=True, env={**os.environ, **GIT_ENV})
    c.check("the remote has refs/heads/md-comments", remote_branch.returncode == 0)
    c.check("neither working tree has changes", a.git("status", "--porcelain").strip() == "" and b.git("status", "--porcelain").strip() == "")
    a.render("both-comments")


def run_at_once(c: Ctx, repo: Repo, commands: list[tuple[str, ...]], allowed: set[int]) -> list[tuple[int, str, str]]:
    """Starts every command before waiting for any, records each, and returns
    (exit code, stdout, stderr) in the order given."""
    if not c.built.ok:
        c.events.append(Event("run", "marq-comments (at once)", False, c.built.reason, -1, where=repo.name))
        raise ScenarioFailed("marq-comments is not built (see Build above)")
    procs = [subprocess.Popen([str(BINARY), *args], cwd=repo.path, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              text=True, env={**os.environ, **GIT_ENV}) for args in commands]
    results = []
    for args, proc in zip(commands, procs):
        out, err = proc.communicate(timeout=60)
        shown = "marq-comments " + " ".join(a if re.fullmatch(r"[\w./:=@-]+", a) else repr(a) for a in args)
        ok = proc.returncode in allowed
        c.events.append(Event("run", shown, ok, "started together with the others", proc.returncode, out, err, repo.name))
        if not ok:
            raise ScenarioFailed(f"exit code {proc.returncode}: {shown}")
        results.append((proc.returncode, out, err))
    return results


def summary(t: dict) -> dict:
    """What every clone must agree on: id, folded state, state changes in fold
    order, and the replies in order."""
    return {"id": t["annotation"]["id"], "state": t["state"],
            "changes": [s["id"] for s in t["stateChanges"]],
            "replies": [summary(r) for r in t["replies"]]}


@scenario("11-three-clones-decide", "Three clones decide in parallel and converge",
          "Before any sync, A resolves a comment and accepts a suggestion, B resolves and reopens the same comment, "
          "and C rejects the suggestion A accepted. After syncing in a mixed order, all three clones fold to the same "
          "states and list the same history, and listing warns that the suggestion was both accepted and rejected.")
def three_clones_decide(c: Ctx) -> None:
    remote = c.tmp / "remote.git"
    subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(remote)], check=True,
                   capture_output=True, env={**os.environ, **GIT_ENV})
    a = Repo.create(c, "clone-a", remote)
    b = Repo.clone(c, "clone-b", remote)
    b.configure("Ana", "ana@example.com")
    cc = Repo.clone(c, "clone-c", remote)
    cc.configure("Ben", "ben@example.com")
    line, _ = a.find("reloading")
    cid = new_id(c, a.cli("comment", DOC, "--line", str(line), "--text", "reloading", "-m", "Is this the right word?"), "comment")
    line, _ = a.find("awkward")
    sid = new_id(c, a.cli("suggest", DOC, "--line", str(line), "--text", "awkward", "--replace", "peculiar",
                          "-m", "A better word."), "suggestion")
    for repo in (a, b, cc):
        repo.cli("sync")
    c.note("Every clone now has both annotations. No clone syncs again until each has decided.")
    a.cli("resolve", cid)
    a.cli("accept", sid)
    b.cli("resolve", cid)
    b.cli("reopen", cid)
    cc.cli("reject", sid)
    for repo in (cc, b, a, cc, b):
        repo.cli("sync")
    agreed = [summary(t) for t in a.threads()]
    for repo in (b, cc):
        c.check(f"{repo.name} lists the same threads, states and history as clone-a",
                [summary(t) for t in repo.threads()] == agreed)
    t = a.thread(sid)
    c.check("the suggestion has both an accepted and a rejected change", sorted(states(t)) == ["accepted", "rejected"],
            str(states(t)))
    c.check("the suggestion's state is the last change in (created, id) order",
            t["state"] == sorted(t["stateChanges"], key=lambda s: (s["created"], s["id"]))[-1]["marq:state"])
    c.check("the comment has three changes: A's resolve, B's resolve and B's reopen", len(a.thread(cid)["stateChanges"]) == 3)
    for repo in (a, b, cc):
        repo.cli("list", DOC)
        c.check(f"listing in {repo.name} warns on standard error that the suggestion was decided both ways",
                f"suggestion {sid} was both accepted and rejected" in c.events[-1].err, c.events[-1].err)
    c.check("clone-a's markdown has the accepted edit", "Note the peculiar slugs" in a.read())
    c.check("clone-c's markdown is unchanged, as it rejected", "Note the awkward slugs" in cc.read())
    for repo in (a, b, cc):
        repo.render(repo.name)


@scenario("12-twelve-processes", "Twelve processes write at once, and nothing is lost",
          "Twelve comments started together all land, one commit each, in one line of history. Then six replies and "
          "six resolves of one comment start together: every reply lands, and exactly one resolve succeeds, because "
          "a process checks the state and writes the change while it holds the lock.")
def twelve_processes(c: Ctx) -> None:
    r = Repo.create(c, "work")
    lines = [n for n, text in enumerate(r.read().split("\n"), start=1) if text.strip()][:12]
    c.check("setup: the markdown has twelve non-blank lines", len(lines) == 12, str(len(lines)))
    results = run_at_once(c, r, [("comment", DOC, "--line", str(n), "-m", f"Comment from process {k + 1}.")
                                 for k, n in enumerate(lines)], {0})
    ids = [new_id(c, out, f"process {k + 1}") for k, (_, out, _) in enumerate(results)]
    threads = r.threads()
    c.check("all twelve comments are listed",
            sorted(t["annotation"]["id"].replace("urn:uuid:", "")[:8] for t in threads) == sorted(ids))
    first, second = ids[0], ids[1]
    commands = [("reply", first, "-m", f"Reply {k + 1}.") for k in range(6)] + [("resolve", second)] * 6
    results = run_at_once(c, r, commands, {0, 1})
    replies, resolves = results[:6], results[6:]
    c.check("every reply succeeded", all(code == 0 for code, _, _ in replies))
    c.check("exactly one resolve succeeded", sum(code == 0 for code, _, _ in resolves) == 1,
            str([code for code, _, _ in resolves]))
    c.check("the other five were refused because the comment was already resolved",
            all("it is resolved, not open" in err for code, _, err in resolves if code != 0))
    threads = r.threads()
    c.check("the first comment has six replies", len(r.thread(first, threads)["replies"]) == 6)
    c.check("the second comment has one state change", states(r.thread(second, threads)) == ["resolved"])
    count = r.git("rev-list", "--count", "refs/heads/md-comments").strip()
    c.check("md-comments has one commit per write: 12 comments, 6 replies, 1 resolve", count == "19", count)
    merges = r.git("rev-list", "--merges", "refs/heads/md-comments").strip()
    c.check("the history is one line, with no merge", merges == "", merges)
    r.git("fsck", "--strict", "--no-progress")
    c.check("git fsck is clean", True)
    r.render("twelve")


# --- running and reporting ---------------------------------------------------


@dataclass
class Result:
    slug: str
    title: str
    expectation: str
    ok: bool
    reason: str
    events: list[Event]
    seconds: float


def run_scenario(slug: str, title: str, expectation: str, fn, built: Build) -> Result:
    ctx = Ctx(slug, built)
    start = time.time()
    ok, reason = True, ""
    try:
        fn(ctx)
    except ScenarioFailed as failure:
        ok, reason = False, str(failure)
    except Exception as error:  # a bug in the scenario or the harness, not the CLI
        ok, reason = False, f"harness error: {type(error).__name__}: {error}"
    finally:
        ctx.cleanup()
    return Result(slug, title, expectation, ok, reason, ctx.events, time.time() - start)


def clip(text: str) -> str:
    return text if len(text) <= TRUNCATE else text[:TRUNCATE] + f"\n... {len(text) - TRUNCATE} more characters"


CSS = """
:root{--bg:#fff;--fg:#1b1f24;--muted:#59636e;--line:#d1d9e0;--card:#f6f8fa;--ok:#1a7f37;--bad:#cf222e;--okbg:#dafbe1;--badbg:#ffebe9}
@media (prefers-color-scheme:dark){:root{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--line:#30363d;--card:#151b23;--ok:#3fb950;--bad:#f85149;--okbg:#12261e;--badbg:#2d1214}}
body{margin:0;padding:24px 16px 64px;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,sans-serif}
main{max-width:960px;margin:0 auto}h1{font-size:24px;margin:0 0 4px}h2{font-size:18px;margin:0}
p.meta{color:var(--muted);margin:0 0 20px}table{border-collapse:collapse;width:100%;margin-bottom:28px}
td,th{padding:6px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}
.pass{color:var(--ok);font-weight:600}.fail{color:var(--bad);font-weight:600}
details{border:1px solid var(--line);border-radius:8px;margin:0 0 16px;background:var(--card)}
details>summary{padding:12px 16px;cursor:pointer;list-style:none}details>summary::-webkit-details-marker{display:none}
details.passing{border-left:4px solid var(--ok)}details.failing{border-left:4px solid var(--bad)}
.body{padding:0 16px 16px}.why{margin:4px 0 12px;color:var(--muted)}
.event{margin:8px 0}pre{margin:4px 0 0;padding:8px 10px;background:var(--bg);border:1px solid var(--line);border-radius:6px;overflow-x:auto;font:12.5px/1.4 ui-monospace,Menlo,monospace;white-space:pre-wrap}
.cmd{font-weight:600}.tag{font-size:12px;color:var(--muted)}
.check{padding:3px 8px;border-radius:4px}.check.ok{background:var(--okbg)}.check.bad{background:var(--badbg)}
.note{color:var(--muted);font-style:italic}iframe{width:100%;height:420px;border:1px solid var(--line);border-radius:6px;background:#fff;margin-top:4px}
.fail-reason{color:var(--bad)}code{font:12.5px ui-monospace,Menlo,monospace}
"""


def render_page(results: list[Result], built: Build) -> str:
    passed = sum(r.ok for r in results)
    parts = [f"<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>"
             f"<title>marq-comments acceptance</title><style>{CSS}</style><main>",
             "<h1>marq-comments acceptance</h1>",
             f"<p class=meta>{passed} of {len(results)} scenarios pass &middot; {time.strftime('%Y-%m-%d %H:%M:%S')} &middot; "
             f"example: <code>example-docs/test.md</code> &middot; design: <code>doc/comments-design.md</code> section 7.1</p>"]
    state = "pass" if built.ok else "fail"
    parts.append(f"<p>Build: <span class={state}>{'ok' if built.ok else 'FAILED'}</span>: {html.escape(built.reason)}</p>")
    if built.log and not built.ok:
        parts.append(f"<pre>{html.escape(clip(built.log))}</pre>")
    parts.append("<table><tr><th>#<th>Scenario<th>Result<th>First failure")
    for r in results:
        parts.append(f"<tr><td>{r.slug[:2]}<td><a href='#{r.slug}'>{html.escape(r.title)}</a>"
                     f"<td class={'pass' if r.ok else 'fail'}>{'PASS' if r.ok else 'FAIL'}"
                     f"<td>{html.escape(r.reason)}")
    parts.append("</table>")
    for r in results:
        parts.append(f"<details id='{r.slug}' class='{'passing' if r.ok else 'failing'}' {'' if r.ok else 'open'}>"
                     f"<summary><h2>{r.slug[:2]}. {html.escape(r.title)} "
                     f"<span class={'pass' if r.ok else 'fail'}>{'PASS' if r.ok else 'FAIL'}</span> "
                     f"<span class=tag>{r.seconds:.1f}s</span></h2></summary><div class=body>")
        parts.append(f"<p class=why>Expected: {html.escape(r.expectation)}</p>")
        if not r.ok:
            parts.append(f"<p class=fail-reason>Stopped: {html.escape(r.reason)}</p>")
        for e in r.events:
            if e.kind == "note":
                parts.append(f"<div class='event note'>{html.escape(e.text)}</div>")
            elif e.kind == "check":
                parts.append(f"<div class='event check {'ok' if e.ok else 'bad'}'>{'&#10003;' if e.ok else '&#10007;'} "
                             f"{html.escape(e.text)}{(': <code>' + html.escape(clip(e.detail)) + '</code>') if e.detail and not e.ok else ''}</div>")
            elif e.kind == "run":
                parts.append(f"<div class=event><span class=tag>{html.escape(e.where)} &middot; {'exit ' + str(e.code) if e.code >= 0 else 'not run'} &middot; {html.escape(e.detail)}</span>"
                             f"<pre class=cmd>$ {html.escape(e.text)}</pre>")
                if e.out.strip():
                    parts.append(f"<pre>{html.escape(clip(e.out))}</pre>")
                if e.err.strip():
                    parts.append(f"<pre class=err>{html.escape(clip(e.err))}</pre>")
                parts.append("</div>")
            elif e.kind == "render":
                parts.append(f"<div class=event><span class=tag>render: {html.escape(e.text)} &middot; "
                             f"<a href='{html.escape(e.file)}'>open on its own</a></span>"
                             f"<iframe src='{html.escape(e.file)}' loading=lazy></iframe></div>")
        parts.append("</div></details>")
    parts.append("</main>")
    return "".join(parts)


def main() -> int:
    parser = argparse.ArgumentParser(description="Acceptance run for marq-comments.")
    parser.add_argument("--open", action="store_true", help="open the page when done")
    parser.add_argument("--no-build", action="store_true", help="do not run cargo build first")
    parser.add_argument("--only", metavar="SLUG", help="run only scenarios whose slug contains SLUG")
    args = parser.parse_args()

    if not EXAMPLE.exists():
        print(f"missing {EXAMPLE}", file=sys.stderr)
        return 2
    if shutil.which("git") is None:
        print("git is not installed", file=sys.stderr)
        return 2

    shutil.rmtree(OUT, ignore_errors=True)
    OUT.mkdir(parents=True)

    built = build(args.no_build)
    print("marq-comments acceptance")
    print(f"  build: {'ok' if built.ok else 'FAILED'}: {built.reason}")

    results = []
    for slug, title, expectation, fn in SCENARIOS:
        if args.only and args.only not in slug:
            continue
        result = run_scenario(slug, title, expectation, fn, built)
        results.append(result)
        print(f"  {slug[:2]} {title:<58} {'PASS' if result.ok else 'FAIL'}" + ("" if result.ok else f"  {result.reason[:90]}"))

    page = OUT / "index.html"
    page.write_text(render_page(results, built), encoding="utf-8")
    passed = sum(r.ok for r in results)
    print(f"\n{passed} of {len(results)} scenarios pass.\nPage: {page}")

    if args.open:
        opener = "open" if platform.system() == "Darwin" else "xdg-open"
        if shutil.which(opener):
            subprocess.run([opener, str(page)], check=False)
        else:
            print(f"({opener} not found: open the page by hand)")
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
