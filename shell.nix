{
  nixpkgs ? <nixpkgs>,
  system ? builtins.currentSystem,
  pkgs ? import nixpkgs { inherit system; },
  pimalaya ? import (fetchTarball "https://github.com/pimalaya/nix/archive/master.tar.gz"),
  ...
}@args:

let
  inherit (pkgs)
    cargo-deny
    maddy
    openssl
    sqlite
    ;
  shell = pimalaya.mkShell (removeAttrs args [ "pimalaya" ]);

in
shell.overrideAttrs (prev: {
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
    openssl
    sqlite
  ];

  # The IMAP server without UIDPLUS tests/without_uidplus.rs starts.
  NEVEREST_MADDY = "${maddy}/bin/maddy";

  buildInputs = (prev.buildInputs or [ ]) ++ [
    cargo-deny
    openssl
    sqlite
  ];
})
