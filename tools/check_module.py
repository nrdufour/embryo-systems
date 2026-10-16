#!/usr/bin/env python3
"""Gate for nix/module.nix: a NixOS system with the module enabled, as the
flake's lib.serviceUnit evaluates it (nothing built), has the service the
daemon needs: the command, its user, its socket's group and umask, its
directories, its restart."""

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
    "RestrictAddressFamilies=AF_UNIX",
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
    print(f"module: the service has all {len(WANTED)} things it needs")


if __name__ == "__main__":
    main()
