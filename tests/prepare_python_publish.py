"""Select intact checked distributions and safely resume a partial PyPI release.

Keep artifacts in separate directories: independently built ABI3 wheels share
filenames, and concurrent extraction into one directory can corrupt their bytes.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

from check_release_version import check_versions

ROOT = Path(__file__).resolve().parents[1]
HOSTS = ("ubuntu-latest", "macos-latest", "windows-latest")
PYTHONS = ("3.10", "3.14")
EXPECTED_ARTIFACTS = {
    f"checked-python-wheel-{host}-{python}" for host in HOSTS for python in PYTHONS
} | {"checked-python-sdist"}
EXPECTED_CHECKS = {
    f"check / Wheel / {host} / Python {python}" for host in HOSTS for python in PYTHONS
} | {"check / Package version agreement", "check / Source distribution / Python 3.12"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_source(run, jobs, artifacts, repository, version, tag_sha):
    require(
        run["path"] == ".github/workflows/publish-pypi.yml", "Wrong source workflow"
    )
    require(
        run["repository"]["full_name"] == repository
        and run["head_repository"]["full_name"] == repository,
        "Source repository differs or is a fork",
    )
    require(run["event"] == "push", "Source must be a tag-triggered release")
    require(run["head_branch"] == f"v{version}", "Source tag differs")
    require(run["head_sha"] == tag_sha, "Source commit differs from release tag")
    require(run["status"] == "completed", "Source run must be completed")
    checks = [job for job in jobs if job["name"] != "publish"]
    require(
        sorted(job["name"] for job in checks) == sorted(EXPECTED_CHECKS),
        "Source must contain the complete expected package-check matrix",
    )
    require(
        all(job["conclusion"] == "success" for job in checks),
        "Every original package check must have succeeded",
    )
    require(
        sorted(artifact["name"] for artifact in artifacts)
        == sorted(EXPECTED_ARTIFACTS),
        "Source must contain exactly the expected checked distributions",
    )
    for artifact in artifacts:
        require(not artifact["expired"], "Source artifact expired")
        require(
            artifact["workflow_run"]["id"] == run["id"]
            and artifact["workflow_run"]["head_sha"] == tag_sha,
            "Artifact belongs to another run or commit",
        )
        require(
            re.fullmatch(r"sha256:[a-f0-9]{64}", artifact.get("digest", "")),
            "Artifact must have a SHA-256 digest",
        )
    return ",".join(str(artifact["id"]) for artifact in artifacts)


def read_json(url, headers=None):
    request = urllib.request.Request(
        url, headers={"User-Agent": "cifmi-release", **(headers or {})}
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def source_artifacts(run_id, repository, version):
    require(re.fullmatch(r"[1-9][0-9]*", run_id), "Invalid source run ID")
    token = os.environ["GH_TOKEN"]

    def api(endpoint):
        return read_json(
            f"https://api.github.com/repos/{repository}/{endpoint}",
            {
                "Authorization": f"Bearer {token}",
                "Accept": "application/vnd.github+json",
            },
        )

    def listing(endpoint, key):
        items = []
        for page in range(1, 1000):
            data = api(f"{endpoint}?per_page=100&page={page}")
            items.extend(data[key])
            if len(items) >= data["total_count"]:
                return items
            require(data[key], "Incomplete GitHub pagination")
        raise ValueError("Too many GitHub results")

    run = api(f"actions/runs/{run_id}")
    require(run["id"] == int(run_id), "GitHub returned another run")
    return validate_source(
        run,
        listing(f"actions/runs/{run_id}/jobs", "jobs"),
        listing(f"actions/runs/{run_id}/artifacts", "artifacts"),
        repository,
        version,
        api(f"commits/v{version}")["sha"],
    )


def checked_files(directory, version):
    require(
        {entry.name for entry in directory.iterdir()} == EXPECTED_ARTIFACTS,
        "Download each expected artifact into its own directory",
    )
    candidates = {}
    host_names = {}
    for artifact_name in sorted(EXPECTED_ARTIFACTS):
        files = list((directory / artifact_name).iterdir())
        require(
            len(files) == 1 and files[0].is_file(),
            "Expected one distribution per artifact",
        )
        path = files[0]
        if artifact_name == "checked-python-sdist":
            require(path.name == f"cifmi-{version}.tar.gz", "Wrong source distribution")
        else:
            require(
                path.name.startswith(f"cifmi-{version}-") and path.suffix == ".whl",
                "Wrong wheel version or filename",
            )
            host = next(host for host in HOSTS if f"-{host}-" in artifact_name)
            require(
                host_names.setdefault(host, path.name) == path.name,
                "ABI3 wheel names differ across Python versions",
            )
            data = path.read_bytes()
            end = data.rfind(b"PK\x05\x06")
            require(
                end >= 0
                and end + 22 <= len(data)
                and end + 22 + int.from_bytes(data[end + 20 : end + 22], "little")
                == len(data),
                f"Wheel has trailing data or no ZIP end record: {path}",
            )
            with zipfile.ZipFile(path) as wheel:
                require(wheel.testzip() is None, f"Wheel CRC failure: {path}")
        candidates.setdefault(path.name, []).append(path)
    require(
        len(candidates) == len(HOSTS) + 1, "Expected three unique wheels and one sdist"
    )
    return candidates


def plan_uploads(candidates, metadata, version):
    published = {}
    if metadata is not None:
        require(
            metadata["info"]["name"] == "cifmi"
            and metadata["info"]["version"] == version,
            "PyPI returned another package or version",
        )
        for file in metadata["urls"]:
            name = file["filename"]
            require(
                name in candidates and name not in published,
                "Unexpected published filename",
            )
            require(not file.get("yanked", False), "Published file is yanked")
            digest = file["digests"]["sha256"]
            require(
                any(
                    hashlib.sha256(path.read_bytes()).hexdigest() == digest
                    for path in candidates[name]
                ),
                f"Published {name} differs from every original checked artifact",
            )
            published[name] = digest
    # Artifact names sort Python 3.10 before 3.14. Copy one intact original;
    # an already published file may match either checked build and stays untouched.
    return [
        paths[0] for name, paths in sorted(candidates.items()) if name not in published
    ]


def prepare(directory, output, version):
    candidates = checked_files(directory, version)
    try:
        metadata = read_json(f"https://pypi.org/pypi/cifmi/{version}/json")
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        metadata = None
    pending = plan_uploads(candidates, metadata, version)
    require(
        not output.exists() or not any(output.iterdir()),
        "Output directory must be empty",
    )
    output.mkdir(parents=True, exist_ok=True)
    for path in pending:
        shutil.copyfile(path, output / path.name)
        print(f"Selected {path}")
    print(
        f"Verified {len(candidates) - len(pending)} existing files; {len(pending)} uploads remain"
    )
    return len(pending)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("source").add_argument("run_id")
    selection = commands.add_parser("prepare")
    selection.add_argument("directory", type=Path)
    selection.add_argument("output", type=Path)
    args = parser.parse_args()
    version = check_versions(ROOT)
    if args.command == "source":
        key = "artifact_ids"
        value = source_artifacts(args.run_id, os.environ["GITHUB_REPOSITORY"], version)
    else:
        key = "pending_count"
        value = prepare(args.directory, args.output, version)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"{key}={value}\n")


if __name__ == "__main__":
    main()
