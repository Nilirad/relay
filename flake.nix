{
  description = "Nixos config flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    # For devenv shells using Rust.
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs @ {
    self,
    flake-parts,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux"];

      perSystem = {
        config,
        pkgs,
        system,
        ...
      }: {
        _module.args.pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [inputs.rust-overlay.overlays.default];
        };

        devShells.default = let
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = ["rust-src" "rust-analyzer"];
          };

          rustNightly = pkgs.rust-bin.nightly.latest.default.override {
            extensions = ["rust-src" "rust-analyzer"];
          };

          cargo-wrapper = pkgs.writeShellScriptBin "cargo" ''
            if [ "$1" = "+nightly" ]; then
              shift
              export RUSTC="${rustNightly}/bin/rustc"
              export RUSTDOC="${rustNightly}/bin/rustdoc"
              exec "${rustNightly}/bin/cargo" "$@"
            elif [ "$1" = "+stable" ]; then
              shift
              exec "${rustToolchain}/bin/cargo" "$@"
            else
              exec "${rustToolchain}/bin/cargo" "$@"
            fi
          '';

          rustc-wrapper = pkgs.writeShellScriptBin "rustc" ''
            if [ "$1" = "+nightly" ]; then
              shift
              exec "${rustNightly}/bin/rustc" "$@"
            elif [ "$1" = "+stable" ]; then
              shift
              exec "${rustToolchain}/bin/rustc" "$@"
            else
              exec "${rustToolchain}/bin/rustc" "$@"
            fi
          '';

          rustdoc-wrapper = pkgs.writeShellScriptBin "rustdoc" ''
            if [ "$1" = "+nightly" ]; then
              shift
              exec "${rustNightly}/bin/rustdoc" "$@"
            elif [ "$1" = "+stable" ]; then
              shift
              exec "${rustToolchain}/bin/rustdoc" "$@"
            else
              exec "${rustToolchain}/bin/rustdoc" "$@"
            fi
          '';
        in
          pkgs.mkShell {
            name = "default";

            nativeBuildInputs = with pkgs; [
              pkg-config
              mold
              # Rust Wrappers
              cargo-wrapper
              rustc-wrapper
              rustdoc-wrapper
              # Rust Standard Fallbacks
              rustToolchain
            ];

            buildInputs = with pkgs;
              [
                cargo-edit
                cargo-watch
                cargo-udeps
                sqlx-cli
              ];

            packages = with pkgs; [
              sqlite-interactive
            ];

            shellHook = ''
              echo "Rust dev shell ready (Rust $(rustc --version))"
              export DATABASE_URL="sqlite://relay.db?mode=rwc"
            '';
          };
      };
    };
}
