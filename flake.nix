{
  description = "CLI to synchronize, backup and restore emails";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-24.05";
    gitignore = {
      url = "github:hercules-ci/gitignore.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    fenix = {
      # https://github.com/nix-community/fenix/pull/145
      # url = "github:nix-community/fenix";
      url = "github:soywod/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, gitignore, fenix, naersk, ... }:
    let
      inherit (nixpkgs) lib;
      inherit (gitignore.lib) gitignoreSource;

      crossSystems = {
        x86_64-linux = {
          x86_64-linux = {
            rustTarget = "x86_64-unknown-linux-musl";
          };

          aarch64-linux = rec {
            rustTarget = "aarch64-unknown-linux-musl";
            runner = { pkgs, neverest }: "${pkgs.qemu}/bin/qemu-aarch64 ${neverest}";
            mkPackage = { system, pkgs }: package:
              let
                inherit (mkPkgsCross system rustTarget) stdenv;
                cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
              in
              package // {
                TARGET_CC = cc;
                CARGO_BUILD_RUSTFLAGS = package.CARGO_BUILD_RUSTFLAGS ++ [ "-Clinker=${cc}" ];
              };
          };

          x86_64-windows = {
            rustTarget = "x86_64-pc-windows-gnu";
            runner = { pkgs, neverest }:
              let wine = pkgs.wine.override { wineBuild = "wine64"; };
              in "${wine}/bin/wine64 ${neverest}.exe";
            mkPackage = { system, pkgs }: package:
              let
                inherit (pkgs.pkgsCross.mingwW64) stdenv windows;
                cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
              in
              package // {
                depsBuildBuild = [ stdenv.cc windows.pthreads ];
                TARGET_CC = cc;
                CARGO_BUILD_RUSTFLAGS = package.CARGO_BUILD_RUSTFLAGS ++ [ "-Clinker=${cc}" ];
              };
          };
        };

        aarch64-linux = {
          aarch64-linux = {
            rustTarget = "aarch64-unknown-linux-musl";
          };
        };

        x86_64-darwin = {
          x86_64-darwin = {
            rustTarget = "x86_64-apple-darwin";
            mkPackage = { pkgs, ... }: package:
              let inherit (pkgs.darwin.apple_sdk.frameworks) AppKit Cocoa;
              in package // {
                buildInputs = [ Cocoa ];
                NIX_LDFLAGS = "-F${AppKit}/Library/Frameworks -framework AppKit";
              };
          };

          # FIXME: https://github.com/NixOS/nixpkgs/issues/273442
          aarch64-darwin = {
            rustTarget = "aarch64-apple-darwin";
            runner = { pkgs, neverest }: "${pkgs.qemu}/bin/qemu-aarch64 ${neverest}";
            mkPackage = { system, pkgs }: package:
              let
                inherit ((mkPkgsCross system "aarch64-darwin").pkgsStatic) stdenv darwin;
                inherit (darwin.apple_sdk.frameworks) AppKit Cocoa;
                cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
              in
              package // {
                buildInputs = [ Cocoa ];
                NIX_LDFLAGS = "-F${AppKit}/Library/Frameworks -framework AppKit";
                TARGET_CC = cc;
                CARGO_BUILD_RUSTFLAGS = package.CARGO_BUILD_RUSTFLAGS ++ [ "-Clinker=${cc}" ];
              };
          };
        };

        aarch64-darwin = {
          aarch64-darwin = {
            rustTarget = "aarch64-apple-darwin";
            mkPackage = { pkgs, ... }: package:
              let inherit (pkgs.darwin.apple_sdk.frameworks) AppKit Cocoa;
              in package // {
                buildInputs = [ Cocoa ];
                NIX_LDFLAGS = "-F${AppKit}/Library/Frameworks -framework AppKit";
              };
          };
        };
      };

      eachBuildSystem = lib.genAttrs (builtins.attrNames crossSystems);

      mkPkgsCross = buildSystem: crossSystem: import nixpkgs {
        system = buildSystem;
        crossSystem.config = crossSystem;
      };

      mkToolchain = import ./rust-toolchain.nix fenix;

      mkApp = { pkgs, buildSystem, targetSystem ? buildSystem }:
        let
          neverest = lib.getExe self.packages.${buildSystem}.${targetSystem};
          wrapper = crossSystems.${buildSystem}.${targetSystem}.runner or (_: neverest) { inherit pkgs neverest; };
          program = lib.getExe (pkgs.writeShellScriptBin "neverest" "${wrapper} $@");
          app = { inherit program; type = "app"; };
        in
        app;

      mkApps = buildSystem:
        let
          pkgs = import nixpkgs { system = buildSystem; };
          mkApp' = targetSystem: _: mkApp { inherit pkgs buildSystem targetSystem; };
          defaultApp = mkApp { inherit pkgs buildSystem; };
          apps = builtins.mapAttrs mkApp' crossSystems.${buildSystem};
        in
        apps // { default = defaultApp; };

      mkPackage = { pkgs, buildSystem, targetSystem ? buildSystem }:
        let
          targetConfig = crossSystems.${buildSystem}.${targetSystem};
          toolchain = mkToolchain.fromTarget {
            inherit pkgs buildSystem;
            targetSystem = targetConfig.rustTarget;
          };
          rust = naersk.lib.${buildSystem}.override {
            cargo = toolchain;
            rustc = toolchain;
          };
          mkPackage' = targetConfig.mkPackage or (_: p: p);
          neverest = "./neverest";
          runner = targetConfig.runner or (_: neverest) { inherit pkgs neverest; };
          package = mkPackage' { inherit pkgs; system = buildSystem; } {
            name = "neverest";
            src = gitignoreSource ./.;
            strictDeps = true;
            doCheck = false;
            auditable = false;
            nativeBuildInputs = with pkgs; [ pkg-config ];
            CARGO_BUILD_TARGET = targetConfig.rustTarget;
            CARGO_BUILD_RUSTFLAGS = [ "-Ctarget-feature=+crt-static" ];
            postInstall = ''
              export WINEPREFIX="$(mktemp -d)"

              mkdir -p $out/bin/share/{applications,completions,man,services}
              cp assets/neverest.desktop $out/bin/share/applications/
              cp assets/neverest-watch@.service $out/bin/share/services/

              cd $out/bin
              ${runner} man ./share/man
              ${runner} completion bash > ./share/completions/neverest.bash
              ${runner} completion elvish > ./share/completions/neverest.elvish
              ${runner} completion fish > ./share/completions/neverest.fish
              ${runner} completion powershell > ./share/completions/neverest.powershell
              ${runner} completion zsh > ./share/completions/neverest.zsh
              tar -czf neverest.tgz neverest* share
              ${pkgs.zip}/bin/zip -r neverest.zip neverest* share

              mv share ../
              mv neverest.tgz neverest.zip ../
            '';
          };
        in
        rust.buildPackage package;

      mkPackages = buildSystem:
        let
          pkgs = import nixpkgs { system = buildSystem; };
          mkPackage' = targetSystem: _: mkPackage { inherit pkgs buildSystem targetSystem; };
          defaultPackage = mkPackage { inherit pkgs buildSystem; };
          packages = builtins.mapAttrs mkPackage' crossSystems.${buildSystem};
        in
        packages // { default = defaultPackage; };

      mkDevShells = buildSystem:
        let
          pkgs = import nixpkgs { system = buildSystem; };
          rust-toolchain = mkToolchain.fromFile { inherit buildSystem; };
          defaultShell = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [
              # Nix
              nixd
              nixpkgs-fmt

              # Rust
              rust-toolchain
              cargo-watch

              # Email env
              gnupg
              gpgme
              notmuch
            ];
          };
        in
        { default = defaultShell; };

    in
    {
      apps = eachBuildSystem mkApps;
      packages = eachBuildSystem mkPackages;
      devShells = eachBuildSystem mkDevShells;
    };
}
