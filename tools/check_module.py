#!/usr/bin/env python3
"""Gate for nix/module.nix: a NixOS system with the module enabled, as the
flake's lib.serviceUnit evaluates it, has the service the daemon needs:
the command, its user, its socket's group and umask, its directories, its
restart, and every line that confines it. Then the package the service
runs is built, as a host would build it, and its command answers."""

import subprocess
import sys

WANTED = [
    "ExecStart=",
    "embryo-systems daemon --socket /run/embryo-systems/daemon.sock --store /var/lib/embryo-systems --pace real",
    "--factor 2.0",
    "User=embryo-systems",
    "Group=embryo-systems",
    "UMask=0007",
    "StateDirectory=embryo-systems",
    "RuntimeDirectory=embryo-systems",
    "Restart=on-failure",
    "RuntimeDirectoryMode=0750",
    "ReadWritePaths=/var/lib/embryo-systems",
    "RestrictAddressFamilies=AF_UNIX",
    "NoNewPrivileges=true",
    "ProtectSystem=strict",
    "ProtectHome=true",
    "PrivateTmp=true",
    "PrivateDevices=true",
    "ProtectKernelTunables=true",
    "ProtectKernelModules=true",
    "ProtectControlGroups=true",
    "RestrictNamespaces=true",
    "LockPersonality=true",
    "MemoryDenyWriteExecute=true",
    "SystemCallArchitectures=native",
]


def main():
    unit = subprocess.run(
        ["nix", "eval", "--raw", ".#lib.serviceUnit"],
        capture_output=True,
        text=True,
    )
    if unit.returncode != 0:
        print(unit.stderr)
        sys.exit(1)
    missing = [w for w in WANTED if w not in unit.stdout]
    if missing:
        print("the module's service lacks:", *missing, sep="\n  ")
        print(unit.stdout)
        sys.exit(1)
    # The package: the module's default, built outside the dev shell.
    built = subprocess.run(
        ["nix", "build", "--no-link", "--print-out-paths", ".#default"],
        capture_output=True,
        text=True,
    )
    if built.returncode != 0:
        print("the package the module runs does not build:")
        print(built.stderr[-4000:])
        sys.exit(1)
    command = built.stdout.strip() + "/bin/embryo-systems"
    if f"ExecStart={command} daemon" not in unit.stdout:
        print(f"the service does not run the package built, {command}")
        sys.exit(1)
    usage = subprocess.run([command], capture_output=True, text=True)
    if "embryo-systems daemon --socket" not in usage.stdout + usage.stderr:
        print(f"{command} does not answer with its usage:")
        print(usage.stdout + usage.stderr)
        sys.exit(1)
    print(f"module: the service has all {len(WANTED)} things it needs, and its package builds")


if __name__ == "__main__":
    main()
