{
  description = "Tackly Android and sync server development shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "aarch64-darwin";
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.${system}.default = pkgs.mkShellNoCC {
        packages = with pkgs; [
          cargo
          jdk21
          just
          python3
          rustc
        ];

        shellHook = ''
          export ANDROID_HOME="''${ANDROID_HOME:-$HOME/Library/Android/sdk}"
          export ANDROID_SDK_ROOT="$ANDROID_HOME"
          export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
          export FLUTTER_BIN="''${FLUTTER_BIN:-/opt/homebrew/bin/flutter}"
        '';
      };
    };
}
