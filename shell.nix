let
  moz_overlay = import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz);
  nixpkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
in
  with nixpkgs;
  mkShell {
    buildInputs = [
      # (nixpkgs.latest.rustChannels.stable.rust.override {
      #  extensions = ["rust-src"];
      # })
      nixpkgs.rustc
      nixpkgs.cargo
      nixpkgs.rustfmt
      nixpkgs.rust-analyzer
      nixpkgs.libiconv
      nixpkgs.llvmPackages.lldb
    ];
  }
