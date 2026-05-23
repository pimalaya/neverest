{
  nixpkgs ? <nixpkgs>,
  pimalaya ? import (fetchTarball "https://github.com/pimalaya/nix/archive/master.tar.gz"),
  ...
}@args:

pimalaya.mkDefault (
  {
    src = ./.;
    version = "2.0.0-alpha.1";
    mkPackage = (
      {
        lib,
        pkgs,
        buildPackages,
        rustPlatform,
        defaultFeatures,
        features,
      }:
      (pkgs.callPackage "${nixpkgs}/pkgs/by-name/ne/neverest/package.nix" {
        inherit lib rustPlatform;
        buildNoDefaultFeatures = !defaultFeatures;
        buildFeatures = lib.splitString "," features;
      })
      # HACK: needed until new derivation available on nixpkgs's
      # master branch
      .overrideAttrs
        {
          postInstall =
            let
              inherit (pkgs) stdenv;
              emulator = stdenv.hostPlatform.emulator buildPackages;
              exe = stdenv.hostPlatform.extensions.executable;
            in
            lib.optionalString (lib.hasInfix "wine" emulator) ''
              export WINEPREFIX="''${WINEPREFIX:-$(mktemp -d)}"
              mkdir -p $WINEPREFIX
            ''
            + ''
              mkdir -p $out/share/{applications,completions,man}
              ${emulator} "$out"/bin/neverest${exe} manuals "$out"/share/man
              ${emulator} "$out"/bin/neverest${exe} completions -d "$out"/share/completions bash elvish fish powershell zsh
            ''
            + lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
              installManPage "$out"/share/man/*
            ''
            + lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
              installShellCompletion --bash "$out"/share/completions/neverest.bash
              installShellCompletion --fish "$out"/share/completions/neverest.fish
              installShellCompletion --zsh "$out"/share/completions/_neverest
            '';
        }
    );
  }
  // removeAttrs args [ "pimalaya" ]
)
