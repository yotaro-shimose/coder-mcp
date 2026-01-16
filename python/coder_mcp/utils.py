from pathlib import Path


def chmod_recursive(path: Path):
    for p in path.rglob("*"):
        p.chmod(0o777)
