{
  description = "A CLI for BeerCan";

  inputs = {
    nixpkgs.url = "nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils";
    devenv.url = "github:cachix/devenv";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };
  outputs = {
    self,
    nixpkgs,
    flake-utils,
    devenv,
    fenix,
  } @ inputs:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};
    in rec {
      devShells.default = devenv.lib.mkShell {
        inherit inputs pkgs;
        modules = [
          ({
            config,
            pkgs,
            ...
          }: {
            packages = with pkgs; [sqlite git diesel-cli openssl protobuf tokio-console openssl pkg-config];
            languages.rust = {
              enable = true;
              channel = "stable";
            };
            pre-commit.hooks = {
              rustfmt.enable = true;
            };
          })
        ];
      };
    });
}
