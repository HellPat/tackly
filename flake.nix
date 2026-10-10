{
  description = "Tackly: shared family tasks (Rust, Dioxus, SQLite)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            clippy
            dioxus-cli
            just
            pkg-config
            rustc
            rustfmt
            sqlite
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux (with pkgs; [
            gtk3
            libsoup_3
            openssl
            webkitgtk_4_1
            xdotool
          ]);

          shellHook = ''
            export TACKLY_DEV_SHELL=1
          '';
        };
      });
    };
}
