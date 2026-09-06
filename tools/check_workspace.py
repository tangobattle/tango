#!/usr/bin/env python3
"""Check shared dependency and game-feature conventions (Python 3.11+)."""

from collections import defaultdict
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def check(root: Path) -> list[str]:
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    shared = workspace["dependencies"]
    external = defaultdict(set)
    errors = []
    game_sets = {}

    for path in sorted(root.glob("*/Cargo.toml")):
        manifest = tomllib.loads(path.read_text())
        name = manifest["package"]["name"]
        label = str(path.relative_to(root))
        if not manifest.get("lints", {}).get("workspace"):
            errors.append(f"{label}: inherit workspace lints")

        sections = [manifest, *manifest.get("target", {}).values()]
        for section in sections:
            for table in DEPENDENCY_TABLES:
                for dependency, spec in section.get(table, {}).items():
                    if isinstance(spec, str):
                        spec = {"version": spec}
                    if "path" in spec:
                        continue
                    external[dependency].add(label)
                    if dependency in shared and not spec.get("workspace"):
                        errors.append(f"{label}: {dependency} must inherit its workspace dependency")

        features = manifest.get("features", {})
        if "gamesupport-all" in features:
            games = {feature for feature in features if feature.startswith("gamesupport-")} - {"gamesupport-all"}
            enabled = features["gamesupport-all"]
            if len(enabled) != len(set(enabled)) or set(enabled) != games:
                errors.append(f"{label}: gamesupport-all must list every game feature exactly once")
            game_sets[name] = games

    for dependency, manifests in sorted(external.items()):
        if len(manifests) > 1 and dependency not in shared:
            errors.append(f"{dependency}: used by multiple crates; declare its version in workspace.dependencies")
    for dependency in sorted(shared.keys() - external.keys()):
        errors.append(f"workspace.dependencies: {dependency} is unused")
    if game_sets and any(games != next(iter(game_sets.values())) for games in game_sets.values()):
        errors.append("desktop, browser, and library game features must agree")
    return errors


def main() -> int:
    errors = check(ROOT)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("Workspace dependencies, lints, and game features are consistent.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
