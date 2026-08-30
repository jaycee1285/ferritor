{
  description = "ferritor — egui file browser + text editor — migrated";

  # To activate:
  #   cp flake.nix flake.nix.bak && cp flake.nix.proposed flake.nix
  #   nix flake update config
  # To revert:
  #   cp flake.nix.bak flake.nix && rm flake.nix.bak

  inputs = {
    config.url = "github:jaycee1285/config";
	nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
#    nixpkgs.follows = "config/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, config, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        libs = config.lib.runtimeLibs pkgs;
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        nativeDeps = libs.egui;
        cleanSrc = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let base = builtins.baseNameOf path; in
            !builtins.elem base [ "target" "result" ".git" ];
        };
      in {
        libs.declared = {
          categories = [ "egui" ];
          local = [];
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain pkg-config
          ] ++ nativeDeps;

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath nativeDeps;
        };

        packages.default = rustPlatform.buildRustPackage {
  pname = "ferritor";
  version = "0.1.0";
  src = cleanSrc;

  cargoHash = "sha256-OdTqNKxZtA39r5cQYKFAvW9P+Fv/LE2racTH1MCnH9c=";

  nativeBuildInputs = with pkgs; [ pkg-config makeWrapper ];
  buildInputs = nativeDeps;

  postFixup = ''
    wrapProgram $out/bin/ferritor \
      --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath nativeDeps}
  '';
};

        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };
      });
}
