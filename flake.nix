{
  description = "openwar3: open-source reimplementation of the Warcraft III engine";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer ];
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          # The viewer loads Vulkan and the window system libraries at run
          # time.
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux
            (pkgs.lib.makeLibraryPath (with pkgs; [
              vulkan-loader
              wayland
              libxkbcommon
              libx11
              libxcursor
              libxi
              libxrandr
            ]));
        };
      });
    };
}
