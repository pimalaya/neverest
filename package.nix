# TODO: move this to nixpkgs
# This file aims to be an up-to-date replacement on master for the nixpkgs derivation.

{ lib
, pkg-config
, rustPlatform
, fetchFromGitHub
, buildPackages
, stdenv
, apple-sdk
, installShellFiles
, installShellCompletions ? stdenv.buildPlatform.canExecute stdenv.hostPlatform
, installManPages ? stdenv.buildPlatform.canExecute stdenv.hostPlatform
, notmuch
, buildNoDefaultFeatures ? false
, buildFeatures ? [ ]
, withNoDefaultFeatures ? buildNoDefaultFeatures
, withFeatures ? buildFeatures
}@args:

let
  version = "1.0.0";
  hash = "sha256-3PSJyhxrOCiuHUeVHO77+NecnI5fN5EZfPhYizuYvtE=";
  cargoHash = "sha256-i5or8oBtjGqOfTfwB7dYXn/OPgr5WEWNEvC0WdCCG+c=";

  noDefaultFeatures =
    lib.warnIf
      (args ? buildNoDefaultFeatures)
      "buildNoDefaultFeatures is deprecated in favour of withNoDefaultFeatures and will be removed in the next release"
      withNoDefaultFeatures;

  features =
    lib.warnIf
      (args ? buildFeatures)
      "buildFeatures is deprecated in favour of withFeatures and will be removed in the next release"
      withFeatures;

in
rustPlatform.buildRustPackage {
  inherit version cargoHash;

  pname = "neverest";

  src = fetchFromGitHub {
    inherit hash;
    owner = "pimalaya";
    repo = "neverest";
    rev = "v${version}";
  };

  useFetchCargoVendor = true;

  buildNoDefaultFeatures = noDefaultFeatures;
  buildFeatures = features;


  nativeBuildInputs = [ pkg-config ]
    ++ lib.optional (installManPages || installShellCompletions) installShellFiles;

  buildInputs = [ ]
    ++ lib.optional stdenv.hostPlatform.isDarwin apple-sdk
    ++ lib.optional (builtins.elem "notmuch" withFeatures) notmuch;

  doCheck = false;
  auditable = false;

  # unit tests only
  cargoTestFlags = [ "--lib" ];

  postInstall = let emulator = stdenv.hostPlatform.emulator buildPackages; in
    ''
      mkdir -p $out/share/{completions,man}
      ${emulator} "$out"/bin/neverest man "$out"/share/man
      ${emulator} "$out"/bin/neverest completion bash > "$out"/share/completions/neverest.bash
      ${emulator} "$out"/bin/neverest completion elvish > "$out"/share/completions/neverest.elvish
      ${emulator} "$out"/bin/neverest completion fish > "$out"/share/completions/neverest.fish
      ${emulator} "$out"/bin/neverest completion powershell > "$out"/share/completions/neverest.powershell
      ${emulator} "$out"/bin/neverest completion zsh > "$out"/share/completions/neverest.zsh
    ''
    + lib.optionalString installManPages ''
      installManPage "$out"/share/man/*
    ''
    + lib.optionalString installShellCompletions ''
      installShellCompletion "$out"/share/completions/neverest.{bash,fish,zsh}
    '';

  meta = with lib; {
    description = "CLI to manage emails";
    mainProgram = "neverest";
    homepage = "https://github.com/pimalaya/neverest";
    changelog = "https://github.com/pimalaya/neverest/blob/v${version}/CHANGELOG.md";
    license = licenses.mit;
    maintainers = with maintainers; [ soywod ];
  };
}
