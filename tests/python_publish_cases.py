"""PyPI artifact-selection and release-recovery regressions (Python 3.12)."""

import copy
import hashlib
import re
import tempfile
import unittest
import urllib.error
import zipfile
from pathlib import Path
from unittest.mock import patch

import prepare_python_publish as publish


class SelectionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.artifacts = self.root / "artifacts"
        self.version = "0.1.0"
        for artifact in publish.EXPECTED_ARTIFACTS:
            directory = self.artifacts / artifact
            directory.mkdir(parents=True)
            if artifact == "checked-python-sdist":
                (directory / "cifmi-0.1.0.tar.gz").write_bytes(
                    b"checked source archive"
                )
            else:
                host = next(host for host in publish.HOSTS if host in artifact)
                filename = f"cifmi-0.1.0-cp310-abi3-{host}.whl"
                with zipfile.ZipFile(directory / filename, "w") as wheel:
                    wheel.writestr("payload", artifact)
        self.candidates = publish.checked_files(self.artifacts, self.version)

    def metadata(self, paths):
        return {
            "info": {"name": "cifmi", "version": self.version},
            "urls": [
                {
                    "filename": path.name,
                    "digests": {
                        "sha256": hashlib.sha256(path.read_bytes()).hexdigest()
                    },
                    "yanked": False,
                }
                for path in paths
            ],
        }

    def test_first_release_selects_one_intact_310_wheel_per_platform(self):
        selected = publish.plan_uploads(self.candidates, None, self.version)
        self.assertEqual(len(selected), 4)
        self.assertTrue(
            all(
                path.parent.name.endswith("3.10")
                for path in selected
                if path.suffix == ".whl"
            )
        )

    def test_partial_release_preserves_matching_alternate_build(self):
        macos = next(
            paths for name, paths in self.candidates.items() if "macos" in name
        )
        selected = publish.plan_uploads(
            self.candidates, self.metadata([macos[1]]), self.version
        )
        self.assertEqual(len(selected), 3)
        self.assertFalse(any("macos" in path.name for path in selected))

    def test_complete_release_has_no_uploads(self):
        metadata = self.metadata([paths[0] for paths in self.candidates.values()])
        self.assertEqual(
            publish.plan_uploads(self.candidates, metadata, self.version), []
        )

    def test_digest_mismatch_aborts_before_creating_output(self):
        metadata = self.metadata([next(iter(self.candidates.values()))[0]])
        metadata["urls"][0]["digests"]["sha256"] = "0" * 64
        output = self.root / "dist"
        with patch.object(publish, "read_json", return_value=metadata):
            with self.assertRaisesRegex(ValueError, "differs from every original"):
                publish.prepare(self.artifacts, output, self.version)
        self.assertFalse(output.exists())

    def test_registry_errors_do_not_mean_a_new_release(self):
        for status in (401, 403, 500):
            error = urllib.error.HTTPError(
                "https://pypi.org", status, "failure", {}, None
            )
            with (
                self.subTest(status=status),
                patch.object(publish, "read_json", side_effect=error),
            ):
                with self.assertRaises(urllib.error.HTTPError):
                    publish.prepare(self.artifacts, self.root / "dist", self.version)

    def test_404_copies_original_bytes(self):
        error = urllib.error.HTTPError("https://pypi.org", 404, "absent", {}, None)
        output = self.root / "dist"
        with patch.object(publish, "read_json", side_effect=error):
            self.assertEqual(publish.prepare(self.artifacts, output, self.version), 4)
        for path in output.iterdir():
            self.assertEqual(
                path.read_bytes(), self.candidates[path.name][0].read_bytes()
            )

    def test_existing_unrelated_or_yanked_files_fail(self):
        for mutation in ("name", "version", "filename", "yanked"):
            metadata = self.metadata([next(iter(self.candidates.values()))[0]])
            if mutation in ("name", "version"):
                metadata["info"][mutation] = "unexpected"
            elif mutation == "filename":
                metadata["urls"][0]["filename"] = "unexpected.whl"
            else:
                metadata["urls"][0]["yanked"] = True
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                publish.plan_uploads(self.candidates, metadata, self.version)

    def test_duplicate_merge_tail_is_rejected(self):
        wheel = next(self.artifacts.rglob("*.whl"))
        wheel.write_bytes(wheel.read_bytes() + b"\x00\x00\x00")
        with self.assertRaisesRegex(ValueError, "trailing data"):
            publish.checked_files(self.artifacts, self.version)

    def test_corrupted_wheel_crc_is_rejected(self):
        wheel = next(self.artifacts.rglob("*.whl"))
        data = bytearray(wheel.read_bytes())
        data[data.index(b"checked-python")] ^= 1
        wheel.write_bytes(data)
        with self.assertRaisesRegex(ValueError, "CRC failure"):
            publish.checked_files(self.artifacts, self.version)

    def test_missing_or_extra_artifact_is_rejected(self):
        (self.artifacts / "unexpected").mkdir()
        with self.assertRaisesRegex(ValueError, "each expected artifact"):
            publish.checked_files(self.artifacts, self.version)


class SourceTests(unittest.TestCase):
    def source(self):
        return {
            "run": {
                "id": 123,
                "path": ".github/workflows/publish-pypi.yml",
                "repository": {"full_name": "jcwang587/cifmi"},
                "head_repository": {"full_name": "jcwang587/cifmi"},
                "event": "push",
                "head_branch": "v0.1.0",
                "head_sha": "tag-commit",
                "status": "completed",
            },
            "jobs": [
                {"name": name, "conclusion": "success"}
                for name in publish.EXPECTED_CHECKS
            ]
            + [{"name": "publish", "conclusion": "failure"}],
            "artifacts": [
                {
                    "id": index + 1,
                    "name": name,
                    "expired": False,
                    "digest": "sha256:" + "a" * 64,
                    "workflow_run": {"id": 123, "head_sha": "tag-commit"},
                }
                for index, name in enumerate(sorted(publish.EXPECTED_ARTIFACTS))
            ],
            "repository": "jcwang587/cifmi",
            "version": "0.1.0",
            "tag_sha": "tag-commit",
        }

    def test_failed_publish_with_all_checks_passed_can_recover(self):
        self.assertEqual(publish.validate_source(**self.source()), "1,2,3,4,5,6,7")

    def test_recovery_rejects_unverified_sources(self):
        source = self.source()
        changes = [
            ("run", "path", "another-workflow.yml"),
            ("run", "head_repository", {"full_name": "another/repository"}),
            ("run", "event", "workflow_dispatch"),
            ("run", "head_branch", "main"),
            ("run", "head_sha", "another-commit"),
            ("run", "status", "in_progress"),
            ("job", "conclusion", "failure"),
            ("job", "name", "another-check"),
            ("artifact", "name", "another-artifact"),
            ("artifact", "expired", True),
            ("artifact", "digest", ""),
            ("artifact", "workflow_run", {"id": 124, "head_sha": "tag-commit"}),
        ]
        for kind, key, value in changes:
            changed = copy.deepcopy(source)
            target = (
                changed["run"]
                if kind == "run"
                else changed[{"job": "jobs", "artifact": "artifacts"}[kind]][0]
            )
            target[key] = value
            with self.subTest(kind=kind, key=key), self.assertRaises(ValueError):
                publish.validate_source(**changed)

    def test_expected_matrix_matches_workflow(self):
        workflow = (publish.ROOT / ".github/workflows/check-python.yml").read_text()
        for key, expected in (
            ("os", publish.HOSTS),
            ("python-version", publish.PYTHONS),
        ):
            match = re.search(rf"^        {key}: \[([^\]]+)\]$", workflow, re.MULTILINE)
            self.assertIsNotNone(match)
            self.assertEqual(
                tuple(value.strip().strip('"') for value in match[1].split(",")),
                expected,
            )


if __name__ == "__main__":
    unittest.main()
