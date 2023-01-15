{
  description = "A cool library for interacting with the Block Exchange Protocol";

  inputs = {
    nixpkgs.url = "nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};
    in {
      defaultPackage = pkgs.stdenv.mkDerivation {
        pname = "bep.rs";
        version = "0.1";
        buildInputs = with pkgs; [cargo rustc git];
      };
      formatter = pkgs.alejandra;
    });
}
