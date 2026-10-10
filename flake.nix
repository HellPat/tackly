{
  description = "Tackly: shared family tasks (Rust, Dioxus, SQLite)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.fenix = {
    url = "github:nix-community/fenix";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, fenix, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system} fenix.packages.${system});
    in
    {
      devShells = forAll (pkgs: fx: {
        default = pkgs.mkShell {
          packages = [
            # Rust with the Android standard libraries, so `just android` works.
            (fx.combine [
              (fx.stable.withComponents [ "cargo" "clippy" "rust-src" "rust-std" "rustc" "rustfmt" ])
              fx.targets.aarch64-linux-android.stable.rust-std
              fx.targets.x86_64-linux-android.stable.rust-std
            ])
          ] ++ (with pkgs; [
            dioxus-cli
            jdk21
            just
            nodejs_22
            pkg-config
            sqlite
            tailwindcss_4  # `just css` compiles the app styles
          ]) ++ pkgs.lib.optionals pkgs.stdenv.isLinux (with pkgs; [
            gtk3
            libsoup_3
            openssl
            webkitgtk_4_1
            xdotool
          ]);

          shellHook = ''
            export TACKLY_DEV_SHELL=1
            export JAVA_HOME="${pkgs.jdk21}"
            # The Android SDK is a host tool, like Android Studio installs it.
            export ANDROID_HOME="''${ANDROID_HOME:-$HOME/Library/Android/sdk}"
            [ -d "$ANDROID_HOME" ] || export ANDROID_HOME="$HOME/Android/Sdk"
            export ANDROID_SDK_ROOT="$ANDROID_HOME"
            if [ -d "$ANDROID_HOME/ndk" ]; then
              export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/$(ls "$ANDROID_HOME/ndk" | sort -V | tail -1)"
              export NDK_HOME="$ANDROID_NDK_HOME"
            fi
            export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
          '';
        };
      });
    };
}
