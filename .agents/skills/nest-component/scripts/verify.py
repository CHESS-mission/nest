#!/usr/bin/env python3
"""Check a component against what NEST expects of it.

NEST binds a component by name, at instantiation, so most ways of getting a port wrong
are invisible to the compiler. Both halves are checked here:

  1. dump the component's world, and derive its ports and environment reads
  2. compare them with the manifest in the component's README.md
  3. run the component in a live simulation, next to the harness, and drive it
  4. report each failure in terms a fix can act on

Usage: verify.py <component-directory> [--wasm PATH] [--no-build] [--quiet]
"""

import argparse
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover
    sys.exit("verify.py must run with PyYAML available, e.g.\n"
             "  uv run --no-project --with pyyaml --with websocket-client python verify.py ...")

try:
    from websocket import create_connection
except ImportError:  # pragma: no cover
    sys.exit("verify.py must run with websocket-client available, e.g.\n"
             "  uv run --no-project --with pyyaml --with websocket-client python verify.py ...")

# WIT type of a port function, as it appears in the world, to the NEST port type
WIT_TO_NEST = {
    "bool": "bool",
    "i32": "int",
    "u32": "int",
    "f32": "float",
    "f64": "float",
    "string": "text",
    "bytes": "bytes",
    "i2c-transaction": "i2c",
    "nest:plugin/types/bytes": "bytes",
    "nest:plugin/types/i2c-transaction": "i2c",
    "vec2": "vec2",
    "vec3": "vec3",
    "quaternion": "quaternion",
    "tensor": "tensor",
}

# A port may be declared with either the short or the qualified type name
def nest_type(wit_type: str):
    wit_type = wit_type.strip()
    if wit_type in WIT_TO_NEST:
        return WIT_TO_NEST[wit_type]
    bare = wit_type.split("/")[-1]
    return WIT_TO_NEST.get(bare)


class Check:
    def __init__(self, name, ok, detail=""):
        self.name, self.ok, self.detail = name, ok, detail


def world_of(wasm_tools, artifact):
    """Return the exports, the world-level imports, the interface imports and the
    world text of a component."""
    out = subprocess.run(
        [wasm_tools, "component", "wit", str(artifact)],
        capture_output=True, text=True, check=False)
    if out.returncode != 0:
        # A core module, not a component: nothing can be said about its world
        raise SystemExit(f"{artifact} is not a component:\n{out.stderr.strip()}")
    exports, imports, interfaces = {}, {}, []
    for declaration in declarations(out.stdout):
        match = re.match(r"\s*(export|import)\s+([A-Za-z0-9_.-]+)\s*:\s*(func\().*",
                         declaration)
        if match:
            kind, name, _ = match.groups()
            # Everything from the opening parenthesis on, which is the signature
            start = declaration.index("func(")
            (exports if kind == "export" else imports)[name] = declaration[start:].rstrip("; ")
            continue
        match = re.match(r"\s*import\s+([A-Za-z0-9_.:@/-]+);", declaration)
        if match:
            interfaces.append(match.group(1))
    return exports, imports, interfaces, out.stdout


def declarations(world):
    """The export and import lines of a world, one entry per declaration.

    `wasm-tools component wit` breaks a long signature over several lines, so a
    declaration is followed to its semicolon rather than read line by line.
    """
    found, pending = [], ""
    for line in world.splitlines():
        if pending:
            line = pending + " " + line.strip()
            pending = ""
        if re.match(r"\s*(export|import)\s+\S+\s*:\s*func\(", line):
            if ";" not in line:
                pending = line
                continue
        found.append(line)
    return found


def parse_arguments(signature):
    """`func(msg: i2c-transaction)` and `func() -> f32`."""
    match = re.match(r"func\(([^)]*)\)(?:\s*->\s*([A-Za-z0-9_:/<>-]+))?", signature.strip())
    if not match:
        return None, None
    params, returns = match.groups()
    types = []
    if params.strip():
        for part in params.split(","):
            types.append(part.split(":")[-1].strip())
    return types, (returns or "").strip() or None


def derive_ports(exports, imports):
    """Ports and environment reads, from the naming convention."""
    ports = {}
    for name, signature in exports.items():
        if name.endswith("-in"):
            ports.setdefault(name[:-3], {})["in"] = signature
    for name, signature in imports.items():
        if name.endswith("-out"):
            ports.setdefault(name[:-4], {})["out"] = signature
        elif name.startswith("read-"):
            ports.setdefault(name[5:], {})["read"] = signature

    result = []
    for name, sides in sorted(ports.items()):
        entry = {"name": name, "sides": sides}
        if "read" in sides:
            entry["kind"] = "environment"
            _, returns = parse_arguments(sides["read"])
            entry["type"] = nest_type(returns) if returns else None
            entry["wit_type"] = returns
        else:
            entry["kind"] = "system"
            sample = sides.get("in") or sides.get("out")
            types, _ = parse_arguments(sample) if sample else (None, None)
            entry["type"] = nest_type(types[0]) if types else None
            entry["wit_type"] = types[0] if types else None
            for side in ("in", "out"):
                if side in sides:
                    signature_types, _ = parse_arguments(sides[side])
                    entry[f"{side}_type"] = (
                        nest_type(signature_types[0]) if signature_types else None)
        result.append(entry)
    return result


def manifest_of(readme):
    text = readme.read_text()
    match = re.match(r"^---\n(.*?\n)---\n", text, re.S)
    if not match:
        raise SystemExit(f"{readme}: no YAML front matter; copy assets/README.md.tmpl")
    return yaml.safe_load(match.group(1)) or {}


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


class Simulation:
    """A nest-server instance with one topology loaded."""

    def __init__(self, server, topology, quiet):
        self.server, self.quiet = server, quiet
        self.directory = tempfile.mkdtemp(prefix="nest-verify-")
        self.port = free_port()
        self.log = open(os.path.join(self.directory, "server.log"), "w")
        self.process = subprocess.Popen(
            [server, "--listen", f"127.0.0.1:{self.port}"],
            stdout=self.log, stderr=subprocess.STDOUT)
        self.messages = []
        try:
            self.wait()
            self.client = create_connection(f"ws://127.0.0.1:{self.port}/ws", timeout=5)
            self.send({"action": "upload_topology", "topology_yaml": topology})
            self.drain(0.5)
        except Exception as error:  # noqa: BLE001 - report, do not traceback
            raise SystemExit(f"could not talk to {server}: {error}")

    def wait(self, seconds=10.0):
        """Wait for the server to accept connections."""
        end = time.time() + seconds
        while time.time() < end:
            with socket.socket() as probe:
                probe.settimeout(0.2)
                try:
                    probe.connect(("127.0.0.1", self.port))
                    return
                except OSError:
                    pass
            time.sleep(0.1)
        raise RuntimeError(f"{self.server} did not listen on port {self.port}")

    def send(self, payload):
        self.client.send(json.dumps(payload))

    def drain(self, seconds):
        end = time.time() + seconds
        self.client.settimeout(0.15)
        while time.time() < end:
            try:
                raw = self.client.recv()
            except Exception as error:  # noqa: BLE001
                if "timeout" not in str(error).lower():
                    break
                continue
            self.messages.append(json.loads(raw))

    def stop(self):
        try:
            self.send({"action": "stop"})
            self.drain(0.5)
        except Exception:  # noqa: BLE001
            pass
        self.client.close()
        self.process.terminate()
        self.process.wait(timeout=10)
        self.log.close()

    def of(self, kind):
        return [m for m in self.messages if m.get("type") == kind]

    def logs(self, component=None, level=None):
        return [m for m in self.of("log")
                if (component is None or m.get("component") == component)
                and (level is None or m.get("level") == level)]

    def until(self, component, seconds):
        """Wait for the component to say something, or for the patience to end.

        A Rust component answers in a few milliseconds. An embedded interpreter can
        take seconds before it fails, and a fixed short wait reports it as healthy.
        """
        end = time.time() + seconds
        while time.time() < end:
            self.drain(0.3)
            if any("Initialization complete" in m.get("message", "")
                   for m in self.logs(component)) or self.of("error"):
                return
        return

    def traps(self):
        """Everything which mentions an unimplemented host call, in any form."""
        return [m for m in self.messages
                if "is not implemented by the NEST host" in m.get("message", "")]

    def events(self, component=None, name=None):
        return [m for m in self.of("event")
                if (component is None or m.get("component") == component)
                and (name is None or m.get("event") == name)]


def build_topology(harness, subject, module, ports, env_defaults, options=None):
    """The component, wired to the harness wherever their port types meet."""
    harness_ports = {"i2c": "i2c", "lines": "bool", "serial": "bytes"}
    wires = []
    subject_ports = []
    for port in ports:
        if port["kind"] != "system":
            continue
        subject_ports.append({"name": port["name"], "type": port["type"]})
        for harness_port, harness_type in harness_ports.items():
            if harness_type == port["type"]:
                wires.append([
                    {"component": subject, "port": port["name"]},
                    {"component": "HARNESS", "port": harness_port},
                ])
                break

    environment = []
    variables = []
    for port in ports:
        if port["kind"] != "environment":
            continue
        name = port["name"]
        variable = port.get("variable", name)
        value = env_defaults.get(port["type"], 0)
        variables.append({"name": variable, "type": port["type"]})
        environment.append({"name": name, "variable": variable, "type": port["type"]})

    topology = {
        "components": [
            {
                "name": subject,
                "type": "wasm",
                "config": {"module": str(module),
                           "options": [[o["name"], str(o.get("sample", ""))]
                                      for o in options or [] if "sample" in o]},
                "ports": {"system": subject_ports, "environment": environment},
            },
            {
                "name": "HARNESS",
                "type": "wasm",
                "config": {"module": str(harness)},
                "ports": {"system": [{"name": n, "type": t} for n, t in harness_ports.items()]},
            },
        ],
        "wires": wires,
    }
    if variables:
        topology["environment_variables"] = variables
    return topology, wires


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("directory", nargs="?", help="component project directory")
    parser.add_argument("--wasm", help="check this artifact instead of the project's")
    parser.add_argument("--no-build", action="store_true", help="skip building")
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()

    directory = os.path.abspath(args.directory or ".")
    config_path = os.path.join(directory, ".nest-component.json")
    config = {}
    if os.path.exists(config_path):
        config = json.load(open(config_path))

    repo = os.environ.get("NEST_REPO") or config.get("repo") or directory
    server = shutil.which("nest-server") or os.path.join(
        repo, "target", "release", "nest-server")
    if not os.path.exists(server):
        raise SystemExit(
            f"nest-server not found (looked in {server}); build it with "
            "cargo build --release -p nest-server, or set NEST_REPO")

    wasm_tools = shutil.which("wasm-tools")
    if not wasm_tools:
        out = subprocess.run(
            ["nix", "build", "--no-link", "--print-out-paths", "--impure", "--expr",
             "let p = import <nixpkgs> {}; in p.wasm-tools"],
            capture_output=True, text=True)
        wasm_tools = os.path.join(out.stdout.strip(), "bin", "wasm-tools")
    if not os.path.exists(wasm_tools):
        raise SystemExit("wasm-tools not found")

    skill = os.environ.get("SKILL_DIR", str(Path(__file__).resolve().parent.parent))
    harness = os.path.join(skill, "assets", "harness", "harness.wasm")
    if not os.path.exists(harness):
        candidates = [
            os.path.join(directory, "harness.wasm"),
            ".agents/skills/nest-component/assets/harness/harness.wasm",
        ]
        harness = next((c for c in candidates if os.path.exists(c)), None)
    if harness is None:
        raise SystemExit("harness.wasm not found; run assets/harness/build.sh")

    checks = []

    artifact = args.wasm or os.path.join(directory, config.get("artifact",
                                        f"build/{config.get('name', 'component')}.wasm"))
    if not args.no_build and not args.wasm:
        build = os.path.join(os.path.dirname(os.path.abspath(__file__)), "build-component.sh")
        if os.path.exists(build):
            result = subprocess.run([build, directory], capture_output=True, text=True)
            if result.returncode != 0:
                checks.append(Check("build", False, result.stdout + result.stderr))
            elif not args.quiet:
                print(result.stdout.strip().splitlines()[-1])
    checks.append(Check("artifact exists", os.path.exists(artifact), artifact))
    if not os.path.exists(artifact):
        return report(checks, args.quiet)

    exports, imports, interfaces, world_text = world_of(wasm_tools, artifact)
    ports = derive_ports(exports, imports)

    # The conventions NEST binds by, checked without any simulation
    for port in ports:
        if port["kind"] != "system":
            continue
        sides = port["sides"]
        if "in" not in sides or "out" not in sides:
            checks.append(Check(
                f"port {port['name']} declares both directions", False,
                f"found {sorted(sides)}; NEST binds `{port['name']}-in` and "
                f"`{port['name']}-out`"))
        elif sides.get("in_type") != sides.get("out_type"):
            checks.append(Check(
                f"port {port['name']} types match in both directions", False,
                f"in is {sides.get('in_type')}, out is {sides.get('out_type')}"))
        else:
            checks.append(Check(f"port {port['name']}", True,
                                f"{port['type']}, bidirectional"))

    known = {"emit", "get-time", "set-timer", "init", "on-command", "on-timeout"}
    unknown = [name for name in list(imports) + interfaces
               if not re.match(r"^(nest:|wasi:)", name)
               and name not in known
               and not name.endswith("-out") and not name.startswith("read-")]
    if unknown:
        checks.append(Check("imports are all recognized", False,
                            f"unexpected: {unknown}"))
    else:
        wasi = sorted(n for n in interfaces if n.startswith("wasi:"))
        note = (f"{len(wasi)} WASI interface import(s), which trap if called"
                if wasi else "imports only nest:plugin, so nothing can trap")
        checks.append(Check("imports are all recognized", True, note))

    readme = os.path.join(directory, "README.md")
    manifest = {}
    if os.path.exists(readme):
        manifest = manifest_of(Path(readme))
        declared = {p["name"]: p for p in manifest.get("ports", []) if isinstance(p, dict)}
        for port in ports:
            if port["kind"] != "system":
                continue
            if port["name"] not in declared:
                checks.append(Check(f"README documents port {port['name']}", False,
                                    "add it to the ports list of the front matter"))
            elif declared[port["name"]]["type"] != port["type"]:
                checks.append(Check(
                    f"README agrees with the binary on port {port['name']}", False,
                    f"README says {declared[port['name']]['type']}, "
                    f"the component says {port['type']}"))
        for name in declared:
            if name not in {p["name"] for p in ports}:
                checks.append(Check(f"README port {name} exists in the binary", False,
                                    "declared but the component neither imports nor exports it"))
        for port in manifest.get("environment", []) or []:
            name = port["name"] if isinstance(port, dict) else port
            if not any(p["kind"] == "environment" and p["name"] == name for p in ports):
                checks.append(Check(f"environment port {name}", False,
                                    f"declared, but the component does not import read-{name}"))
        documented = {c["name"] for c in (manifest.get("commands") or [])
                      if isinstance(c, dict)}
        if "on-command" in exports and not documented and manifest.get("commands") is None:
            checks.append(Check("README documents commands", False,
                                "the commands field of the front matter is missing"))
    else:
        checks.append(Check("component README exists", False,
                            f"{readme}: copy assets/README.md.tmpl and fill it"))

    # The live check: load it, drive it, watch what it says
    simulation = None
    try:
        topology, wires = build_topology(
            harness, "DEVICE", artifact, ports,
            {"bool": True, "int": 1, "float": 1.0, "text": "x", "bytes": [0]},
            manifest.get("options") or [])
        simulation = Simulation(server, yaml.safe_dump(topology, sort_keys=False),
                                args.quiet)
        simulation.send({"action": "start"})
        simulation.until("DEVICE", 8.0)

        errors = simulation.of("error")
        checks.append(Check("loads into NEST", not errors,
                            errors[0]["message"][:300] if errors else
                            f"{len(simulation.logs())} log line(s)"))
        late = []

        # The host logs this once init returns Ok, which is the only sign that a
        # component accepted its options
        initialized = [m for m in simulation.logs("DEVICE")
                       if "Initialization complete" in m.get("message", "")]
        refused = [m["message"] for m in simulation.logs("DEVICE", "error")
                   if "option" in m.get("message", "").lower()
                   or "value" in m.get("message", "").lower()]
        checks.append(Check("accepts the options it documents", bool(initialized),
                            "; ".join(refused) if refused else
                            "" if initialized else "no Initialization complete line"))

        trapped = short_traps(simulation.traps())
        checks.append(Check("no host call traps during start", not trapped,
                            trapped[0]["message"][:220] if trapped else ""))

        # Drive every port the harness shares a type with
        driven = []
        for port in ports:
            if port["kind"] != "system":
                continue
            if port["type"] == "i2c":
                simulation.send({"action": "command", "component": "HARNESS",
                                 "command": "i2c-read",
                                 "arguments": {"addr": 0x48, "register": 0x00, "len": 2}})
                driven.append(("i2c", "i2c-reply"))
            elif port["type"] == "bool":
                simulation.send({"action": "command", "component": "HARNESS",
                                 "command": "lines-set", "arguments": {"value": True}})
                driven.append(("bool", None))
            elif port["type"] == "bytes":
                simulation.send({"action": "command", "component": "HARNESS",
                                 "command": "serial-send",
                                 "arguments": {"bytes": [0x00, 0x01]}})
                driven.append(("bytes", None))
        simulation.drain(1.5)

        trapped = short_traps(simulation.traps())
        checks.append(Check("no host call traps while handling messages", not trapped,
                            trapped[0]["message"][:220] if trapped else ""))

        for port_type, event in driven:
            if event is None:
                continue
            seen = simulation.events("HARNESS", event)
            checks.append(Check(
                f"{port_type} port answers on the bus", bool(seen),
                f"{len(seen)} {event} event(s) from the harness" if seen else
                "the harness sent a transaction and heard nothing back"))

        for command in manifest.get("commands") or []:
            name = command["name"] if isinstance(command, dict) else command
            arguments = command.get("sample", {})
            simulation.send({"action": "command", "component": "DEVICE",
                             "command": name, "arguments": arguments})
            simulation.drain(0.6)
            rejected = [m for m in simulation.logs("DEVICE", "error")
                        if f"Unknown command: {name}" in m.get("message", "")]
            checks.append(Check(f"command {name} is accepted", not rejected,
                                rejected[0]["message"][:200] if rejected else ""))

        trapped = short_traps(simulation.traps())
        checks.append(Check("no host call traps during commands", not trapped,
                            trapped[0]["message"][:220] if trapped else ""))

        simulation.drain(2.5)

        # Anything which went wrong at any moment of the run, including after the
        # start, where a slow interpreter likes to fail
        late = [m for m in simulation.messages if m.get("type") == "error"] + [
            m for m in simulation.logs(level="error") if "Failed to call" in m.get("message", "")]
        checks.append(Check("nothing went wrong during the run", not late,
                            (late[0].get("message") or "")[:300] if late else ""))
    finally:
        if simulation is not None:
            simulation.stop()

    return report(checks, args.quiet)


def short_traps(messages):
    """A trap arrives with a wasm backtrace in front of the reason for it."""
    for message in messages:
        text = message.get("message", "")
        for line in reversed(text.splitlines()):
            if "is not implemented" in line:
                message["message"] = line.strip()
                break
    return messages


def report(checks, quiet):
    failures = [c for c in checks if not c.ok]
    for check in checks:
        if quiet and check.ok:
            continue
        mark = "ok  " if check.ok else "FAIL"
        detail = f"  ({check.detail})" if check.detail else ""
        print(f"[{mark}] {check.name}{detail}")
    print()
    print(f"{'FAILED' if failures else 'PASSED'}: "
          f"{len(checks) - len(failures)}/{len(checks)} checks")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
