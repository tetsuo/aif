"""Prepare versioned source bundles and publish tested Git references."""

import argparse
import copy
from pathlib import Path
import re
import subprocess
import tomllib

VERSION = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def branch_major(branch):
    match = re.fullmatch(r"v(0|[1-9][0-9]*)\.x", branch)
    if not match:
        raise ValueError("release branch must be named vX.x, for example v0.x")
    return int(match[1])


def version_tuple(version):
    match = re.fullmatch(VERSION, version)
    if not match:
        raise ValueError(f"expected a stable major.minor.patch version: {version}")
    return tuple(map(int, match.groups()))


def next_version(branch, impact, tags):
    major = branch_major(branch)
    if impact not in ("patch", "minor"):
        raise ValueError("impact must be patch or minor")
    versions = [
        version_tuple(tag[1:]) for tag in tags
        if re.fullmatch("v" + VERSION, tag) and version_tuple(tag[1:])[0] == major
    ]
    if not versions:
        return f"{major}.{1 if major == 0 else 0}.0"
    _, minor, patch = max(versions)
    return f"{major}.{minor + 1}.0" if impact == "minor" else f"{major}.{minor}.{patch + 1}"


def require_ancestor(ancestor, commit):
    subprocess.run(["git", "merge-base", "--is-ancestor", ancestor, commit], check=True)


def set_version(version):
    manifest = Path("Cargo.toml")
    text = manifest.read_text()
    package = tomllib.loads(text)["package"]
    if package["name"] != "ele":
        raise ValueError("expected the ele package")
    current = package["version"]
    if version_tuple(current) > version_tuple(version):
        raise ValueError(f"manifest version {current} exceeds the planned release {version}")
    if current == version:
        return
    section = re.search(r"(?ms)^\[package\]\n.*?(?=^\[|\Z)", text)
    if not section:
        raise ValueError("expected an explicit [package] section")
    body, count = re.subn(r'(?m)^version\s*=\s*"[^"]+"', f'version = "{version}"', section[0])
    if count != 1:
        raise ValueError("expected one explicit package version")
    expected_lock = copy.deepcopy(tomllib.loads(Path("Cargo.lock").read_text()))
    packages = [p for p in expected_lock["package"] if p["name"] == "ele" and "source" not in p]
    if len(packages) != 1 or packages[0]["version"] != current:
        raise ValueError("manifest and lockfile versions must agree")
    packages[0]["version"] = version
    manifest.write_text(text[:section.start()] + body + text[section.end():])
    subprocess.run(["cargo", "update", "--workspace"], check=True)
    if tomllib.loads(Path("Cargo.lock").read_text()) != expected_lock:
        raise ValueError("version update changed dependencies; refusing to prepare a release")


def prepare(branch, impact, bundle):
    branch_major(branch)
    if run("git", "status", "--porcelain"):
        raise ValueError("release preparation requires a clean working tree")
    base = run("git", "rev-parse", "HEAD")
    if any(re.fullmatch("v" + VERSION, tag) for tag in run("git", "tag", "--points-at", base).splitlines()):
        raise ValueError("this commit already has a release tag; rerun a failed publish job to resume it")
    tags = run("git", "tag", "--list").splitlines()
    version = next_version(branch, impact, tags)
    for tag in tags:
        if re.fullmatch("v" + VERSION, tag) and version_tuple(tag[1:])[0] == branch_major(branch):
            require_ancestor(tag, base)
    set_version(version)
    subprocess.run(["git", "add", "Cargo.toml", "Cargo.lock"], check=True)
    if run("git", "diff", "--cached", "--name-only"):
        subprocess.run([
            "git", "-c", "user.name=github-actions[bot]",
            "-c", "user.email=41898282+github-actions[bot]@users.noreply.github.com",
            "-c", "commit.gpgsign=false", "commit", "-m", f"Release v{version}",
        ], check=True)
    commit = run("git", "rev-parse", "HEAD")
    bundle.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "bundle", "create", str(bundle), "HEAD"], check=True)
    return {"version": version, "tag": f"v{version}", "commit": commit, "base": base}


def push_refs(branch, version, base, commit):
    if branch_major(branch) != version_tuple(version)[0]:
        raise ValueError("release version does not belong to this branch")
    require_ancestor(base, commit)
    if run("git", "rev-parse", "HEAD") != commit:
        raise ValueError("checkout does not match the tested release commit")
    if tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"] != version:
        raise ValueError("manifest does not match the release version")
    if run("git", "status", "--porcelain"):
        raise ValueError("publishing requires a clean working tree")
    tag = f"v{version}"
    remote_tag = run("git", "ls-remote", "--tags", "origin", f"refs/tags/{tag}")
    if remote_tag:
        subprocess.run(["git", "fetch", "origin", f"refs/tags/{tag}"], check=True)
        if run("git", "rev-parse", "FETCH_HEAD^{commit}") != commit:
            raise ValueError("the remote tag belongs to a different commit")
        return
    remote_branch = run("git", "ls-remote", "--heads", "origin", f"refs/heads/{branch}")
    if not remote_branch or remote_branch.split()[0] != base:
        raise ValueError("release branch has moved; start a new workflow run from its current head")
    subprocess.run(["git", "tag", tag, commit], check=True)
    subprocess.run([
        "git", "push", "--atomic", "origin",
        f"{commit}:refs/heads/{branch}", f"refs/tags/{tag}:refs/tags/{tag}",
    ], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--branch", required=True)
    prepare_parser.add_argument("--impact", choices=("patch", "minor"), required=True)
    prepare_parser.add_argument("--bundle", type=Path, required=True)
    prepare_parser.add_argument("--output", type=Path, required=True)
    push_parser = commands.add_parser("push")
    for name in ("branch", "version", "base", "commit"):
        push_parser.add_argument(f"--{name}", required=True)
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            outputs = prepare(args.branch, args.impact, args.bundle)
            with args.output.open("a") as output:
                for name, value in outputs.items():
                    print(f"{name}={value}", file=output)
        else:
            push_refs(args.branch, args.version, args.base, args.commit)
    except (ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release: {error}\n")


if __name__ == "__main__":
    main()
