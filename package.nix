# TODO: move this to nixpkgs
# This file aims to be a replacement for the nixpkgs derivation.

{
  buildFeatures ? [ ],
  buildNoDefaultFeatures ? false,
  buildPackages,
  dbus,
  fetchFromGitHub,
  installManPages ? stdenv.buildPlatform.canExecute stdenv.hostPlatform,
  installShellCompletions ? stdenv.buildPlatform.canExecute stdenv.hostPlatform,
  installShellFiles,
  lib,
  openssl,
  pkg-config,
  rustPlatform,
  stdenv,
}:

let
  nativeTls = builtins.elem "native-tls" buildFeatures;
  notify = !buildNoDefaultFeatures || builtins.elem "notify" buildFeatures;
  dbus' =
    # dbus calls libgcc outline atomics that the static aarch64 link cannot
    # resolve (__aarch64_ldset4_sync & co), so inline them instead.
    if stdenv.hostPlatform.isLinux && stdenv.hostPlatform.isAarch64 then
      dbus.overrideAttrs (old: {
        env = (old.env or { }) // {
          NIX_CFLAGS_COMPILE = (old.env.NIX_CFLAGS_COMPILE or "") + " -mno-outline-atomics";
        };
      })
    else
      dbus;

in
rustPlatform.buildRustPackage (finalAttrs: {
  __structuredAttrs = true;

  inherit buildNoDefaultFeatures;

  pname = "neverest";
  version = "1.0.0-rc";
  cargoHash = "";

  src = fetchFromGitHub {
    owner = "pimalaya";
    repo = finalAttrs.pname;
    tag = "v${finalAttrs.version}";
    hash = "";
  };

  # openssl should not be provided by vendors, not even on windows
  env.OPENSSL_NO_VENDOR = 1;

  # pkg-config hands the linker libdbus but no rpath, leaving a binary that
  # cannot find it: not in postInstall, which runs it, nor once installed.
  env.NIX_LDFLAGS = lib.optionalString (notify && !stdenv.hostPlatform.isWindows) (
    "-rpath " + lib.getLib dbus' + "/lib"
  );

  nativeBuildInputs = [
    pkg-config
    installShellFiles
  ];

  # dbus is provided by vendors on windows
  buildInputs =
    lib.optional nativeTls openssl
    ++ lib.optional (notify && !stdenv.hostPlatform.isWindows) dbus';

  buildFeatures =
    buildFeatures
    # dbus is provided by vendors on windows
    ++ lib.optional (notify && stdenv.hostPlatform.isWindows) "vendored";

  postInstall =
    let
      exe =
        if stdenv.buildPlatform.canExecute stdenv.hostPlatform then
          "$out/bin/${finalAttrs.pname}"
        else
          lib.getExe buildPackages.${finalAttrs.pname};
    in
    ''
      mkdir -p $out/share/{completions,man}
      ${exe} manual -d "$out"/share/man
      ${exe} completion -d "$out"/share/completions bash elvish fish powershell zsh
    ''
    + lib.optionalString installManPages ''
      installManPage "$out"/share/man/*
    ''
    + lib.optionalString installShellCompletions ''
      installShellCompletion --cmd ${finalAttrs.pname} \
        --bash "$out"/share/completions/${finalAttrs.pname}.bash \
        --fish "$out"/share/completions/${finalAttrs.pname}.fish \
        --zsh "$out"/share/completions/_${finalAttrs.pname}
    '';

  # disable impure integration tests: they open sockets against live servers
  cargoTestFlags = [ "--bins" ];

  meta = {
    description = "CLI to synchronize PIM collections: mail, contact, calendar…";
    mainProgram = finalAttrs.pname;
    homepage = "https://github.com/pimalaya/${finalAttrs.pname}";
    changelog = "https://github.com/pimalaya/${finalAttrs.pname}/releases/${finalAttrs.src.tag}";
    license = with lib.licenses; [
      asl20
      mit
    ];
    maintainers = with lib.maintainers; [ soywod ];
  };
})
