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
          extensions = [ "rust-src" "clippy" "rustfmt" "llvm-tools-preview" ];
        };

        # Override crane's default toolchain with our rust-overlay toolchain.
        craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

        # Custom source filter: include Rust-relevant files plus .org files
        # needed by integration tests (embedded at compile time via include_str!).
        orgFilter = path: _type: builtins.match ".*\\.org$" path != null;
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (orgFilter path type) || (craneLib.filterCargoSources path type);
        };

        # Common arguments shared across all crane derivations.
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
        # Cached independently so source-only changes skip dependency rebuild.
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          pname = "org2jsonl-deps";
        });

        # Build the full package (both org2jsonl and jsonl2org binaries).
        org2jsonl = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });

        # Run all tests.
        tests = craneLib.cargoTest (commonArgs // {
          inherit cargoArtifacts;
        });

        # Clippy lint check -- all warnings are errors.
        clippy = craneLib.cargoClippy (commonArgs // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        });

        # Rustfmt formatting check.
        fmt = craneLib.cargoFmt {
          inherit src;
        };

        # Documentation build -- warnings are errors.
        doc = craneLib.cargoDoc (commonArgs // {
          inherit cargoArtifacts;
          cargoDocExtraArgs = "--no-deps";
          RUSTDOCFLAGS = "-D warnings";
        });
      in
      {
        # `nix flake check` runs build, tests, clippy, fmt, and doc checks.
        checks = {
          inherit org2jsonl tests clippy fmt doc;
        };

        packages = {
          inherit org2jsonl;
          default = org2jsonl;
        };

        # Development shell with the Rust toolchain and common tools.
        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            rust-analyzer
            cargo-llvm-cov
            critcmp
            lefthook
          ];
        };
      });
}
