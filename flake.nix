{
  description = "github:moeru-ai/auv";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

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
        ])
        ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (with pkgs; [
          # Only used when developing/building the vendored
          # MediaRemoteAdapter framework on macOS.
          cmake
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

      meta = pkgs: {
        description = "AUV core command-line frontend";
        homepage = "https://github.com/moeru-ai/auv";
        license = pkgs.lib.licenses.asl20;
        mainProgram = "auv";
      };

      # Building the macOS CLI from source requires `swiftc`: the driver and
      # overlay crates compile Swift sidecars through swift-bridge, and the
      # vendored MediaRemoteAdapter framework is built with cmake. A Nix build
      # cannot use Xcode's toolchain, and nixpkgs' `swift` (5.10.1) does not
      # build on aarch64-darwin, so darwin installs the published release
      # artifact instead. Linux keeps building from source.
      darwinArtifacts = {
        aarch64-darwin = {
          target = "aarch64-apple-darwin";
          hash = "sha256-E03tdArJnWnZJzy3y823kLydv+VuuQ3/lnL7gs49f1U=";
        };
        x86_64-darwin = {
          target = "x86_64-apple-darwin";
          hash = "sha256-EcJWA4maNvSQXIhF3W199UBiYFwrLrpthZ8MGVMvrwE=";
        };
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
          default =
            if pkgs.stdenv.isDarwin then
              let
                artifact = darwinArtifacts.${system};
              in
              pkgs.stdenvNoCC.mkDerivation {
                pname = "auv-cli";
                inherit version;
                src = pkgs.fetchurl {
                  url = "https://github.com/moeru-ai/auv/releases/download/v${version}/auv-${artifact.target}.tar.gz";
                  hash = artifact.hash;
                };

                # The tarball contains a single `auv` at its root.
                sourceRoot = ".";
                dontConfigure = true;
                dontBuild = true;

                # Stripping or rewriting the Mach-O would invalidate Apple's
                # code signature, and arm64 macOS refuses to run it then.
                dontFixup = true;

                installPhase = ''
                  runHook preInstall
                  install -Dm755 auv $out/bin/auv
                  runHook postInstall
                '';

                meta = meta pkgs;
              }
            else
              pkgs.rustPlatform.buildRustPackage (
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

                  meta = meta pkgs;
                }
                // env pkgs
              );
        }
      );
    };
}
