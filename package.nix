# TODO: move this to nixpkgs
# This file aims to be a replacement for the nixpkgs derivation.

{ lib
, pkg-config
, rustPlatform
, fetchFromGitHub
, stdenv
, apple-sdk
, installShellFiles
, installShellCompletions ? stdenv.buildPlatform.canExecute stdenv.hostPlatform
, installManPages ? stdenv.buildPlatform.canExecute stdenv.hostPlatform
, notmuch
, buildNoDefaultFeatures ? false
, buildFeatures ? [ ]
}:

let
  version = "1.0.0";
  hash = "sha256-3PSJyhxrOCiuHUeVHO77+NecnI5fN5EZfPhYizuYvtE=";
  cargoHash = "sha256-i5or8oBtjGqOfTfwB7dYXn/OPgr5WEWNEvC0WdCCG+c=";
in

rustPlatform.buildRustPackage {
  inherit version cargoHash;
  inherit buildNoDefaultFeatures buildFeatures;

  pname = "neverest";

  src = fetchFromGitHub {
    inherit hash;
    owner = "pimalaya";
    repo = "neverest";
    rev = "v${version}";
  };

  nativeBuildInputs = [ pkg-config ]
    ++ lib.optional (installManPages || installShellCompletions) installShellFiles;

  buildInputs = [ ]
    ++ lib.optional stdenv.hostPlatform.isDarwin apple-sdk
    ++ lib.optional (builtins.elem "notmuch" buildFeatures) notmuch;

  doCheck = false;
  auditable = false;

  # unit tests only
  cargoTestFlags = [ "--lib" ];

  postInstall = lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    "$out"/bin/neverest man "$out"/share/man
  '' + lib.optionalString installManPages ''
    installManPage "$out"/share/man/*
  '' + lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    "$out"/bin/neverest completion bash > "$out"/share/completions/neverest.bash
    "$out"/bin/neverest completion elvish > "$out"/share/completions/neverest.elvish
    "$out"/bin/neverest completion fish > "$out"/share/completions/neverest.fish
    "$out"/bin/neverest completion powershell > "$out"/share/completions/neverest.powershell
    "$out"/bin/neverest completion zsh > "$out"/share/completions/neverest.zsh
  '' + lib.optionalString installShellCompletions ''
    installShellCompletion "$out"/share/completions/neverest.{bash,fish,zsh}
  '';

  meta = rec {
    description = "CLI to manage emails";
    mainProgram = "neverest";
    homepage = "https://github.com/pimalaya/neverest";
    changelog = "${homepage}/blob/v${version}/CHANGELOG.md";
    license = lib.licenses.mit;
    maintainers = with lib.maintainers; [ soywod ];
  };
}
