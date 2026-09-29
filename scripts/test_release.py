import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

import release

ROOT = Path(__file__).resolve().parent.parent


class VersionTests(unittest.TestCase):
    def test_initial_versions_and_impacts(self):
        self.assertEqual(release.next_version("v0.x", "patch", []), "0.1.0")
        self.assertEqual(release.next_version("v1.x", "minor", []), "1.0.0")
        tags = ["v0.9.9", "v0.10.2", "v0.10.10", "v1.3.0", "v0.11.0-rc.1", "other"]
        self.assertEqual(release.next_version("v0.x", "patch", tags), "0.10.11")
        self.assertEqual(release.next_version("v0.x", "minor", tags), "0.11.0")
        self.assertEqual(release.next_version("v1.x", "patch", tags), "1.3.1")

    def test_invalid_inputs(self):
        for branch in ["master", "v01.x", "v1.2.x", "v1.x\n", "v1.x;echo bad"]:
            with self.subTest(branch=branch), self.assertRaises(ValueError):
                release.next_version(branch, "patch", [])
        with self.assertRaises(ValueError):
            release.next_version("v0.x", "major", [])
        for version in ["01.2.3", "1.2", "1.2.3-rc.1", "1.2.3+build"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.version_tuple(version)


class RepositoryTests(unittest.TestCase):
    def setUp(self):
        directory = ROOT / "target" / "release-script-tests"
        directory.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=directory)
        self.addCleanup(self.temporary.cleanup)
        self.previous = Path.cwd()
        self.addCleanup(os.chdir, self.previous)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        os.chdir(self.repo)
        release.run("git", "init", "-b", "v0.x")
        release.run("git", "config", "user.name", "Release tests")
        release.run("git", "config", "user.email", "release-tests@example.invalid")
        release.run("git", "config", "commit.gpgsign", "false")
        Path(".gitignore").write_text("target/\n")
        Path("Cargo.toml").write_text(
            '[package]\nname = "aip_filter"\nversion = "0.1.0"\nedition = "2024"\n'
        )
        Path("src").mkdir()
        Path("src/lib.rs").write_text("pub fn example() {}\n")
        release.run("cargo", "generate-lockfile", "--offline")
        self.commit("Initial package")
        self.remote = self.root / "origin.git"
        release.run("git", "init", "--bare", "-b", "v0.x", str(self.remote))
        release.run("git", "remote", "add", "origin", str(self.remote))
        release.run("git", "push", "origin", "HEAD:refs/heads/v0.x")

    def commit(self, message):
        release.run("git", "add", ".")
        release.run("git", "commit", "-m", message)
        return release.run("git", "rev-parse", "HEAD")

    def prepare(self, impact="patch"):
        return release.prepare("v0.x", impact, Path("target/source.bundle"))

    def publish(self, result):
        release.push_refs("v0.x", result["version"], result["base"], result["commit"])

    def patch_source(self):
        release.run("git", "tag", "v0.1.0")
        release.run("git", "push", "origin", "v0.1.0")
        Path("src/lib.rs").write_text("pub fn example() -> bool { true }\n")
        self.commit("Change package")
        release.run("git", "push", "origin", "HEAD:refs/heads/v0.x")

    def test_initial_prepare_has_no_remote_side_effects(self):
        before = release.run("git", "ls-remote", "origin")
        result = self.prepare()
        self.assertEqual(result["version"], "0.1.0")
        self.assertEqual(result["base"], result["commit"])
        self.assertEqual(release.run("git", "tag", "--list"), "")
        self.assertEqual(release.run("git", "ls-remote", "origin"), before)
        release.run("git", "bundle", "verify", "target/source.bundle")
        self.assertEqual(release.run("git", "status", "--porcelain"), "")

    def test_patch_updates_manifest_lock_and_bundle_and_publishes_atomically(self):
        self.patch_source()
        before = release.run("git", "ls-remote", "origin")
        result = self.prepare()
        self.assertEqual(result["version"], "0.1.1")
        self.assertNotEqual(result["base"], result["commit"])
        self.assertEqual(release.run("git", "ls-remote", "origin"), before)
        self.assertEqual(tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"], "0.1.1")
        self.assertEqual(tomllib.loads(Path("Cargo.lock").read_text())["package"][0]["version"], "0.1.1")
        release.run("cargo", "check", "--locked", "--offline")
        clone = self.root / "from-bundle"
        release.run("git", "clone", "target/source.bundle", str(clone))
        self.assertEqual(release.run("git", "-C", str(clone), "rev-parse", "HEAD"), result["commit"])
        self.publish(result)
        refs = release.run("git", "ls-remote", "origin")
        self.assertIn(result["commit"] + "\trefs/heads/v0.x", refs)
        self.assertIn(result["commit"] + "\trefs/tags/v0.1.1", refs)
        self.publish(result)
        self.assertEqual(release.run("git", "ls-remote", "origin"), refs)

    def test_already_tagged_dirty_and_downgraded_versions_fail(self):
        Path("untracked").write_text("not committed")
        with self.assertRaises(ValueError):
            self.prepare()
        Path("untracked").unlink()
        release.run("git", "tag", "v0.1.0")
        with self.assertRaises(ValueError):
            self.prepare()
        release.run("git", "tag", "-d", "v0.1.0")
        with self.assertRaises(ValueError):
            release.set_version("0.0.9")

    def test_moved_branch_fails_without_a_tag(self):
        result = self.prepare()
        Path("new-file").write_text("new branch head")
        advanced = self.commit("Advance branch")
        release.run("git", "push", "origin", "HEAD:refs/heads/v0.x")
        release.run("git", "checkout", "--detach", result["commit"])
        with self.assertRaises(ValueError):
            self.publish(result)
        self.assertEqual(release.run("git", "ls-remote", "--tags", "origin"), "")
        self.assertIn(advanced, release.run("git", "ls-remote", "--heads", "origin"))

    def test_conflicting_remote_tag_fails(self):
        self.patch_source()
        result = self.prepare()
        release.run("git", "tag", result["tag"], result["base"])
        release.run("git", "push", "origin", result["tag"])
        with self.assertRaises(ValueError):
            self.publish(result)

    def test_unmerged_release_tags_fail(self):
        base = release.run("git", "rev-parse", "HEAD")
        Path("other-change").write_text("another release branch")
        self.commit("Other branch")
        release.run("git", "tag", "v0.2.0")
        release.run("git", "checkout", "--detach", base)
        with self.assertRaises(subprocess.CalledProcessError):
            self.prepare()

    def test_atomic_push_does_not_advance_branch_when_tag_is_rejected(self):
        self.patch_source()
        result = self.prepare()
        hook = self.remote / "hooks" / "update"
        hook.write_text('#!/bin/sh\ncase "$1" in refs/tags/*) exit 1;; esac\n')
        hook.chmod(0o755)
        with self.assertRaises(subprocess.CalledProcessError):
            self.publish(result)
        self.assertIn(result["base"], release.run("git", "ls-remote", "--heads", "origin"))
        self.assertEqual(release.run("git", "ls-remote", "--tags", "origin", result["tag"]), "")


if __name__ == "__main__":
    unittest.main()
