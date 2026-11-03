"""Install a built distribution in a fresh environment and run the Python suite.

This checks the release artifact, never an editable checkout. Source archives
are installed with pip's build isolation and cache disabled, so their compiled
extension must be built from the archive itself.
"""

import argparse
import json
import os
import subprocess
import tempfile
import venv
from pathlib import Path


def run(command, *, cwd, env):
    print("+", " ".join(map(str, command)), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist-dir", type=Path, required=True)
    parser.add_argument("--kind", choices=("wheel", "sdist"), required=True)
    args = parser.parse_args()
    pattern = "*.whl" if args.kind == "wheel" else "*.tar.gz"
    artifacts = sorted(args.dist_dir.resolve().glob(pattern))
    if len(artifacts) != 1:
        parser.error(
            f"expected exactly one {pattern} in {args.dist_dir}, found {len(artifacts)}"
        )
    artifact = artifacts[0]
    repository = Path(__file__).resolve().parents[1]

    env = os.environ.copy()
    for key in (
        "PYTHONPATH",
        "PYTHONHOME",
        "PYO3_PYTHON",
        "PYO3_ENVIRONMENT_SIGNATURE",
    ):
        env.pop(key, None)

    with tempfile.TemporaryDirectory(prefix="cifmi-package-check-") as temporary:
        work = Path(temporary)
        environment = work / "venv"
        venv.EnvBuilder(with_pip=True).create(environment)
        python = environment / (
            "Scripts/python.exe" if os.name == "nt" else "bin/python"
        )
        env["VIRTUAL_ENV"] = str(environment)
        env["PATH"] = str(python.parent) + os.pathsep + env.get("PATH", "")
        # No wheel cache may satisfy an sdist check without rebuilding it.
        pip = [
            str(python),
            "-I",
            "-m",
            "pip",
            "--isolated",
            "--disable-pip-version-check",
            "--cache-dir",
            str(work / "pip-cache"),
        ]
        run(
            pip + ["install", "--no-input", "--no-cache-dir", str(artifact), "pytest"],
            cwd=work,
            env=env,
        )
        run(pip + ["check"], cwd=work, env=env)

        installed_check = "\n".join(
            [
                "import importlib.metadata, json, pathlib, sys, cifmi",
                "module = pathlib.Path(cifmi.__file__).resolve()",
                "prefix = pathlib.Path(sys.prefix).resolve()",
                "assert module.is_relative_to(prefix), (module, prefix)",
                "assert cifmi.__version__ == importlib.metadata.version('cifmi')",
                "distribution = importlib.metadata.distribution('cifmi')",
                "license_files = distribution.metadata.get_all('License-File', [])",
                "for filename in ('LICENSE', 'THIRD_PARTY_NOTICES.md'):",
                "    assert filename in license_files, (filename, license_files)",
                "    assert any(str(path).endswith('/licenses/' + filename) and distribution.locate_file(path).is_file() for path in distribution.files), filename",
                "print(json.dumps({'python': sys.version, 'module': str(module), 'version': cifmi.__version__}, indent=2))",
            ]
        )
        run([str(python), "-I", "-c", installed_check], cwd=work, env=env)
        run(
            [
                str(python),
                "-I",
                "-m",
                "pytest",
                "--import-mode=importlib",
                str(repository / "tests" / "test_python.py"),
                "-q",
                "-o",
                f"cache_dir={work / 'pytest-cache'}",
            ],
            cwd=work,
            env=env,
        )

        # Install metadata tooling only after runtime tests pass, so its
        # dependencies cannot hide a missing package runtime dependency.
        run(pip + ["install", "--no-input", "twine"], cwd=work, env=env)
        run(
            [str(python), "-I", "-m", "twine", "check", "--strict", str(artifact)],
            cwd=work,
            env=env,
        )

    print(
        json.dumps({"artifact": str(artifact), "kind": args.kind, "status": "passed"})
    )


if __name__ == "__main__":
    main()
