#!/usr/bin/env python3
"""Catch a change that silently undoes recent work on main (issue #218).

#151's squash merge put back the text #150 had replaced, in every file #150
touched, and nobody noticed: its branch held a stale copy of those files.
This compares a change with each of the last WINDOW commits on main. A hunk
of an earlier commit counts as undone when the change deletes most of the
lines that hunk added and puts back most of the lines it removed. If the
change undoes at least MIN_HUNKS hunks and half of an earlier commit, the
check fails, unless the PR's title or body names that commit's PR (#N):
rewriting earlier work is fine, but say so.

  python3 .github/revert-check.py BASE HEAD [--body TEXT]
      Check the change BASE..HEAD against the commits on main up to BASE.
      In CI, BASE is main and HEAD is the PR merged into it, which is
      exactly what a squash merge lands.
  python3 .github/revert-check.py --history [REF]
      Check every first-parent commit of REF (default origin/main) against
      the commits before it: the audit #218 asked for.
"""

import collections
import re
import subprocess
import sys

WINDOW = 30
MIN_HUNKS = 3
SHARE = 0.5


def git(*args):
    return subprocess.run(
        ["git", *args], capture_output=True, text=True, errors="replace", check=True
    ).stdout


def meaningful(line):
    # Braces, blank lines and short words match by chance.
    return len(line) >= 12


def diff(old, new):
    """The hunks of old..new as (file, added lines, removed lines), plus
    the added and removed lines counted per file."""
    hunks, path = [], None
    for line in git("diff", "-U0", "--no-renames", old, new).splitlines():
        if line.startswith(("--- ", "+++ ")):
            if line[4:] != "/dev/null":
                path = line[6:]
            continue
        if line.startswith("@@"):
            hunks.append((path, [], []))
            continue
        if hunks and line[:1] in "+-":
            text = line[1:].strip()
            if meaningful(text):
                hunks[-1][1 if line[0] == "+" else 2].append(text)
    added, removed = collections.Counter(), collections.Counter()
    for path, a, r in hunks:
        added.update((path, s) for s in a)
        removed.update((path, s) for s in r)
    return hunks, added, removed


def undone(change, earlier):
    """The hunks of `earlier` that `change` undoes, and how many it had."""
    _, added, removed = change
    hunks, _, _ = earlier
    out, total = [], 0
    for path, a, r in hunks:
        if len(a) + len(r) < 2:
            continue
        total += 1
        gone = sum(1 for s in a if removed[(path, s)])
        back = sum(1 for s in r if added[(path, s)])
        if (not a or gone >= 0.8 * len(a)) and (not r or back >= 0.5 * len(r)) and gone + back >= 2:
            out.append(path)
    return out, total


def first_parents(ref, n=None):
    args = ["log", "--first-parent", "--format=%H %s", ref]
    if n:
        args.insert(1, f"-{n}")
    return [line.split(" ", 1) for line in git(*args).splitlines()]


def parent(sha):
    """The commit's first parent, or None for the first commit."""
    out = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", f"{sha}^1"], capture_output=True, text=True
    ).stdout.strip()
    return out or None


def pr_number(subject):
    m = re.search(r"\(#(\d+)\)$", subject)
    return m and m.group(1)


def check(base, head, named, label):
    """Report what base..head undoes; True if it's fine."""
    change = diff(base, head)
    ok = True
    for sha, subject in first_parents(base, WINDOW):
        before = parent(sha)
        if not before:
            continue
        hit, total = undone(change, diff(before, sha))
        if len(hit) < MIN_HUNKS or len(hit) < SHARE * total:
            continue
        pr = pr_number(subject)
        files = ", ".join(sorted(set(hit)))
        what = f"{label} undoes {len(hit)} of {total} hunks of {sha[:7]} {subject!r} ({files})"
        if pr and pr in named:
            print(f"ok: {what}; it names #{pr}, so that's meant")
        else:
            ok = False
            print(f"::error::{what}")
    return ok


def main(argv):
    if argv[:1] == ["--history"]:
        ref = argv[1] if len(argv) > 1 else "origin/main"
        commits = list(reversed(first_parents(ref)))
        for sha, subject in commits[1:]:
            # Only the subject is to hand, not the PR's description, so
            # a hit here needs reading to tell a rewrite from a revert.
            named = set(re.findall(r"#(\d+)", subject))
            check(parent(sha), sha, named, f"{sha[:7]} {subject!r}")
        return 0
    if len(argv) < 2:
        print(__doc__)
        return 2
    base, head = argv[0], argv[1]
    body = argv[argv.index("--body") + 1] if "--body" in argv else ""
    named = set(re.findall(r"#(\d+)", body))
    if check(base, head, named, "this change"):
        print("No recent work on main is undone without saying so.")
        return 0
    print(
        "\nThis change puts back text that recent commits on main replaced, as #151 "
        "did to #150 (issue #218). If the branch has a stale copy of those files, "
        "rebase on main and redo the change. If undoing that work is meant, name "
        "its PR (#N) in this PR's description."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
