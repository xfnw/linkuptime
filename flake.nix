{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, flake-utils, nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        name = "linkuptime";
        pkgs = (import nixpkgs) { inherit system; };
        pp = pkgs.python3Packages;
        anyio = pp.anyio.overridePythonAttrs (old: rec {
          version = "2.0.2";
          doCheck = false; # annoyingly slow
          src = pkgs.fetchFromGitHub {
            owner = "agronholm";
            repo = "anyio";
            rev = "refs/tags/${version}";
            hash = "sha256-u0/6hrsS/vGxfSK/oc3ou+O6EeXJ6nfpuJRpUbP7yho=";
          };
        });
        ircrobots = pp.ircrobots.override { inherit anyio; };
      in rec {
        packages."${name}" = pp.buildPythonApplication {
          inherit name;
          propagatedBuildInputs = [ ircrobots ];
          src = ./.;
        };

        defaultPackage = packages."${name}";

        devShell = pkgs.mkShell { buildInputs = [ pkgs.python3 ircrobots ]; };
      });
}

