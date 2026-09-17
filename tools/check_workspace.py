#!/usr/bin/env python3
"""Check shared dependency and game-feature conventions (Python 3.11+)."""

from collections import defaultdict
from pathlib import Path
import re
import sys
import subprocess
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
            if name == "tango-library":
                expected_ui = {"tango-gamesupport/ui", *(f"tango-{game}?/ui" for game in games)}
                if set(features.get("ui", [])) != expected_ui:
                    errors.append(f"{label}: ui must enable editors weakly for every optional game")
                registered = set(re.findall(
                    r'"(gamesupport-[a-z0-9]+)"\s*=>',
                    (root / "tango-library/src/game.rs").read_text(),
                ))
                if registered != games:
                    errors.append(f"{label}: game features must match the registry")
            else:
                for game in games:
                    if features[game] != [f"tango-library/{game}"]:
                        errors.append(f"{label}: {game} must forward only to tango-library")

    for dependency, manifests in sorted(external.items()):
        if len(manifests) > 1 and dependency not in shared:
            errors.append(f"{dependency}: used by multiple crates; declare its version in workspace.dependencies")
    for dependency in sorted(shared.keys() - external.keys()):
        errors.append(f"workspace.dependencies: {dependency} is unused")
    if game_sets and any(games != next(iter(game_sets.values())) for games in game_sets.values()):
        errors.append("desktop, browser, and library game features must agree")
    return errors


def check_boundaries(root: Path) -> list[str]:
    errors = []
    allowed = {
        "tango-match": set(),
        "tango-net-protocol": set(),
        "tango-platform": set(),
        "tango-replay": set(),
        "tango-net": {"tango-platform", "tango-net-protocol"},
        "tango-lobby": {"tango-net", "tango-platform", "tango-net-protocol"},
        "tango-gamesupport": {"tango-match"},
        "tango-gamesupport-common-dataview": {"tango-gamesupport"},
    }
    for crate, permitted in allowed.items():
        manifest = tomllib.loads((root / crate / "Cargo.toml").read_text())
        for section in [manifest, *manifest.get("target", {}).values()]:
            for dependency, spec in section.get("dependencies", {}).items():
                if isinstance(spec, dict) and "path" in spec and dependency not in permitted:
                    errors.append(f"{crate}: forbidden layer dependency {dependency}")
    for path in (root / "tango-session/src").rglob("*.rs"):
        if "std::fs::" in path.read_text():
            errors.append(f"{path.relative_to(root)}: session persistence must use a host adapter")
    for name in ("library", "engine", "link"):
        path = root / f"tango-lite-web/src/{name}.rs"
        if "thread_local!" in path.read_text():
            errors.append(f"{path.relative_to(root)}: state must be owned by an explicit host handle")
    return errors


def check_resolved_boundaries(root: Path) -> list[str]:
    errors = []
    checks = {
        "tango-gamesupport-common-dataview": {"iced", "tango-ui"},
        "tango-library": {"iced", "tango-ui", "tango-session", "tango-lobby", "tango-net"},
        "tango-session": {"iced", "tango-net", "datachannel-wrapper", "tango-signaling"},
        "tango-lobby": {"iced", "tango-library", "tango-session", "tango-match", "mgba", "melonds"},
    }
    for crate, forbidden in checks.items():
        result = subprocess.run(["cargo", "tree", "--locked", "--offline", "--no-default-features", "-p", crate,
                                 "--edges", "normal", "--prefix", "none", "--format", "{p}"],
                                cwd=root, text=True, capture_output=True)
        if result.returncode:
            errors.append(f"{crate}: cannot inspect dependency graph: {result.stderr.strip()}")
            continue
        packages = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
        for dependency in sorted(packages & forbidden):
            errors.append(f"{crate}: headless dependency graph includes {dependency}")
    return errors


def main() -> int:
    errors = check(ROOT) + check_boundaries(ROOT)
    if "--resolved" in sys.argv:
        errors.extend(check_resolved_boundaries(ROOT))
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("Workspace dependencies, features, and architecture boundaries are consistent.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
