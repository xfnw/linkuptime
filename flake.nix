{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, flake-utils, naersk, nixpkgs }:
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
        naersk' = pkgs.callPackage naersk { };
      in rec {
        packages = {
          "${name}" = pp.buildPythonPackage {
            inherit name;
            propagatedBuildInputs = [ ircrobots ];
            src = ./.;
            meta.mainProgram = name;
          };
          importable = pkgs.python3.withPackages (p: [ packages."${name}" ]);
          riir = naersk'.buildPackage { src = ./.; };
        };

        defaultPackage = packages."${name}";

        devShell = pkgs.mkShell {
          buildInputs =
            [ ircrobots pkgs.python3 pkgs.rustc pkgs.cargo pkgs.clippy ];
        };
      });
}

