{
  description = "org2jsonl - Convert Emacs Org-mode files to/from JSONL";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Use the stable Rust toolchain from rust-overlay with extensions
        # needed for development (rust-src for rust-analyzer, clippy, rustfmt).
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" ];
        };

        # Override crane's default toolchain with our rust-overlay toolchain.
        # Using a function form for proper cross-compilation support.
        craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

        # Filter source to only include Rust-relevant files, improving
        # cache hits by excluding irrelevant changes (e.g. README edits).
        src = craneLib.cleanCargoSource ./.;

        # Common arguments shared across all crane derivations to ensure
        # consistency between dependency builds, checks, and the final package.
        commonArgs = {
          inherit src;
          strictDeps = true;

          buildInputs = [ ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
              pkgs.apple-sdk_15
            ];
        };

        # Build only the cargo dependencies as a separate derivation.
        # This is cached independently so that source-only changes do not
        # trigger a full dependency rebuild.
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          pname = "org2jsonl-deps";
        });

        # Build the full package (both org2jsonl and jsonl2org binaries).
        org2jsonl = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          # cargo builds all [[bin]] targets by default, so both
          # org2jsonl and jsonl2org will be present in the output.
        });

        # Clippy lint check -- runs against the full source, reusing
        # the pre-built dependency artifacts for speed.
        clippy = craneLib.cargoClippy (commonArgs // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        });

        # Rustfmt formatting check -- ensures consistent code style.
        # Does not need cargoArtifacts since it only parses source.
        fmt = craneLib.cargoFmt {
          inherit src;
        };
      in
      {
        # `nix flake check` runs clippy, fmt, and a full build.
        checks = {
          inherit org2jsonl clippy fmt;
        };

        packages = {
          inherit org2jsonl;
          default = org2jsonl;
        };

        # Development shell with the Rust toolchain and common tools.
        # craneLib.devShell automatically includes cargo, rustc, and
        # any other binaries from the overridden toolchain.
        devShells.default = craneLib.devShell {
          # Propagate checks so that `inputsFrom` picks up their
          # build inputs (ensures native deps are available in the shell).
          checks = self.checks.${system};

          packages = with pkgs; [
            rust-analyzer
          ];
        };
      });
}
