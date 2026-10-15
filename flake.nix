{
  description = "embryo-systems - Rebuilding François Bouillé's HBDS system from the 1995 polycopié, in Rust";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "embryo-systems";
          version = "0.1.0";
          src = ./.;

          # Hashes come from Cargo.lock, so nothing to update by hand when a
          # dependency is added.
          cargoLock.lockFile = ./Cargo.lock;

          meta = {
            description = "Rebuilding François Bouillé's HBDS system from the 1995 polycopié, in Rust";
            mainProgram = "embryo-systems";
            license = pkgs.lib.licenses.mit;
            platforms = systems;
          };
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          # Stable toolchain from nixpkgs. Nightly, no_std targets or a custom
          # target spec need rust-overlay or fenix instead; add the input then,
          # not before.
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            just # the task runner; `just` alone lists every recipe
            tea # the Forgejo forge's CLI, for pull requests and issues
            forgejo-cli # `fj`: the forge's other CLI
            nixfmt
            (python3.withPackages (ps: [ ps.lark ])) # tools/check_grammar.py
          ];

          env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
