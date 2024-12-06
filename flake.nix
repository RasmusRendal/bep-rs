{
  description = "A CLI for BeerCan";

  inputs = {
    nixpkgs.url = "nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils";
    devenv.url = "github:cachix/devenv/v0.6.3";
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
                version = "latest";
            };
            pre-commit.hooks = {
              rustfmt.enable = true;
            };
          })
        ];
      };
    });
}


        #buildInputs = with pkgs; [cargo rustc git sqlite diesel-cli rust-analyzer protobuf rustfmt tokio-console clippy openssl pkg-config];
