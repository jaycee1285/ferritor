{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        nativeDeps = with pkgs; [
          libxkbcommon
          libGL
          wayland
          libx11
          libxcursor
          libxrandr
          libxi
          vulkan-loader
          fontconfig
          freetype
        ];
        cleanSrc = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let base = builtins.baseNameOf path;
            in !builtins.elem base [ "target" "result" ".git" ];
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            pkg-config

            # egui/eframe native deps
          ] ++ nativeDeps;

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath nativeDeps;
        };

        packages.default = rustPlatform.buildRustPackage {
          pname = "ferritor";
          version = "0.1.0";
          src = cleanSrc;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = nativeDeps;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };
      });
}
