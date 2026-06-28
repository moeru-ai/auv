{
  description = "github:moeru-ai/auv";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "x86_64-darwin"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs =
              (with pkgs; [
                # rust
                rustc
                cargo
                rustfmt
                clippy
                rust-analyzer

                # protobuf
                protobuf
                buf
                protoc-gen-prost
                protoc-gen-tonic
              ])
              ++ [];

            RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
          };

          packages = forAllSystems (
            system:
            let
              pkgs = import nixpkgs { inherit system; };
              version = (fromTOML (builtins.readFile ./Cargo.toml)).package.version;
            in
            {
              default = pkgs.rustPlatform.buildRustPackage {
                inherit version;
                pname = "auv-cli";
                src = ./.;
                cargoLock.lockFile = ./Cargo.lock;
              };
            }
          );
        }
      );
    };
}
