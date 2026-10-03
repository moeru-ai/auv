{
  description = "github:moeru-ai/auv";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.self.submodules = true;

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
      swiftc =
        pkgs:
        pkgs.writeShellScriptBin "swiftc" ''
          unset DEVELOPER_DIR
          unset SDKROOT
          exec /usr/bin/swiftc "$@"
        '';
      codesign =
        pkgs:
        pkgs.writeShellScriptBin "codesign" ''
          exec /usr/bin/codesign "$@"
        '';
      appleClang =
        pkgs:
        pkgs.writeShellScriptBin "auv-apple-clang" ''
          unset DEVELOPER_DIR
          unset SDKROOT
          exec /usr/bin/clang "$@"
        '';
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
                # task runner
                just

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

                # pkg-config
                pkg-config

                # clang
                clang

                # native vendored libraries
                cmake
              ])
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                (swiftc pkgs)
              ];

            buildInputs =
              (with pkgs; [
                openssl
                tesseract
                leptonica
                llvmPackages.libclang
              ])
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (
                with pkgs;
                [
                  libiconv
                ]
              )
              ++ pkgs.lib.optionals pkgs.stdenv.isLinux (
                with pkgs;
                [
                  wayland
                  libxkbcommon
                  libglvnd
                  pipewire
                  libgbm
                  xorg.libxcb
                ]
              );

            RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          };
        }
      );

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "auv";
            inherit version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [
              "--package"
              "auv-cli"
              "--bin"
              "auv"
            ];

            nativeBuildInputs =
              (with pkgs; [
                cmake
                pkg-config
              ])
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                (swiftc pkgs)
                (codesign pkgs)
                (appleClang pkgs)
              ]
              ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
                pkgs.rustPlatform.bindgenHook
              ];

            buildInputs =
              (with pkgs; [
                openssl
              ])
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (
                with pkgs;
                [
                  libiconv
                ]
              )
              ++ pkgs.lib.optionals pkgs.stdenv.isLinux (
                with pkgs;
                [
                  pipewire
                  wayland
                  libxkbcommon
                  tesseract
                  leptonica
                ]
              );

            # NOTICE(nix-macos-framework): mediaremote-adapter defaults to a
            # universal framework, while Nix's compiler runtime is native to
            # the selected host. Build the framework for that host only.
            postPatch = pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              substituteInPlace crates/auv-media-macos/vendor/mediaremote-adapter/CMakeLists.txt \
                --replace-fail 'set(CMAKE_OSX_ARCHITECTURES "x86_64;arm64")' \
                'set(CMAKE_OSX_ARCHITECTURES "${pkgs.stdenv.hostPlatform.darwinArch}")'
            '';

            # NOTICE(nix-swift-linker): Swift native archives carry Apple SDK
            # auto-link metadata that Nix's clang wrapper cannot resolve.
            # Compile dependencies with Nix, but use Apple clang for the final
            # Darwin target link. Remove this when Nix's wrapper supports the
            # selected Swift toolchain and SDK together.
            preBuild = pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              export RUSTFLAGS="$RUSTFLAGS -C linker=${appleClang pkgs}/bin/auv-apple-clang"
            '';

            # NOTICE(nix-package-checks): AUV's tests exercise desktop and
            # local-daemon integration that is not available in a Nix build
            # sandbox. The repository test workflow runs the complete suite.
            doCheck = false;

            meta = {
              description = "Application Use Via command-line frontend";
              homepage = "https://github.com/moeru-ai/auv";
              license = pkgs.lib.licenses.asl20;
              mainProgram = "auv";
              platforms = systems;
            };
          };
        }
      );
    };
}
