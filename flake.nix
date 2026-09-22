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

      # Tools only `nix develop` needs; `buildRustPackage` brings its own pinned
      # Rust toolchain.
      devShellTools =
        pkgs:
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
        ]);

      nativeBuildInputs =
        pkgs:
        (with pkgs; [
          # pkg-config
          pkg-config

          # clang
          clang
        ]);

      # Native libraries the CLI links against. Shared by `nix develop` and
      # `nix build` so the two cannot drift apart.
      buildInputs =
        pkgs:
        (with pkgs; [
          openssl
          tesseract
          leptonica
          llvmPackages.libclang
        ])
        ++ pkgs.lib.optionals pkgs.stdenv.isLinux (with pkgs; [
          wayland
          libglvnd
          pipewire
          libgbm
          libxkbcommon
          xorg.libxcb
        ]);

      env = pkgs: {
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
      };
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell (
            {
              nativeBuildInputs = devShellTools pkgs ++ nativeBuildInputs pkgs;
              buildInputs = buildInputs pkgs;
            }
            // env pkgs
          );
        }
      );

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage (
            {
              inherit version;
              pname = "auv-cli";
              src = ./.;
              cargoLock.lockFile = ./Cargo.lock;

              nativeBuildInputs = nativeBuildInputs pkgs;
              buildInputs = buildInputs pkgs;

              # The workspace default member is the CLI. Its tests drive a live
              # desktop session (Wayland/portal), which the sandbox has none of.
              doCheck = false;

              meta = {
                description = "AUV core command-line frontend";
                homepage = "https://github.com/moeru-ai/auv";
                license = pkgs.lib.licenses.asl20;
                mainProgram = "auv";
              };
            }
            // env pkgs
          );
        }
      );
    };
}
