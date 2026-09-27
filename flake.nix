{
  description = "Terminal roguelike (Caves of Qud-inspired) dev environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      crane,
    }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-darwin" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        inherit (pkgs) lib;
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Cargo sources plus data/ (read by blueprint tests at runtime) and
        # clippy.toml files (the determinism lints in game/ rely on them).
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            ./data
            (lib.fileset.fileFilter (file: file.name == "clippy.toml") ./.)
          ];
        };

        commonArgs = {
          inherit src;
          pname = "roguelike";
          version = "0.1.0";
          strictDeps = true;
        };

        # Dependencies built once and shared by every check.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      in
      {
        checks = {
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              # Nothing consumes a check's own target/ archive; skip storing it.
              doInstallCargoArtifacts = false;
              cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
            }
          );

          test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
              # Nothing consumes a check's own target/ archive; skip storing it.
              doInstallCargoArtifacts = false;
              cargoTestExtraArgs = "--workspace";
            }
          );
        };

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            pkgs.jq # .claude/hooks/block-bash-file-reads.sh
          ];
        };
      }
    );
}
