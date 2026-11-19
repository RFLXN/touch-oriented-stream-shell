{
  description = "Toss Front (PLF02WH, RK3399) 이미지 빌드·백업·플래싱 devShell 및 PC 연동 데몬 toss-connector와 NixOS 모듈";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      inherit (nixpkgs) lib;
      system = "x86_64-linux";
      hostSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      connectorFor =
        sys:
        let
          p = nixpkgs.legacyPackages.${sys};
        in
        p.callPackage ./nix/toss-connector.nix { rust = (rust-overlay.lib.mkRustBin { } p).stable.latest.minimal; };
      pkgs = nixpkgs.legacyPackages.${system};
      rust = (rust-overlay.lib.mkRustBin { } pkgs).stable.latest.minimal.override {
        targets = [ "aarch64-unknown-linux-musl" ];
      };
      inherit (import ./nix/build.nix { inherit pkgs rust; }) buildImage;

      images = buildImage {
        hostname = "toss-front";
        ssh = {
          lan = true;
          authorizedKeys = [ ];
        };
        ntp = {
          enable = true;
          timezone = "Asia/Seoul";
        };
        kiosk = {
          enable = true;
          command = "toss-shell";
        };
        toss = {
          enable = true;
        };
        splash = {
          enable = true;
          background = "#16181b";
          width = 640;
          align = "center";
          offset = 0;
        };
      };
    in
    {
      packages = lib.genAttrs hostSystems (
        sys:
        {
          toss-connector = connectorFor sys;
        }
        // lib.optionalAttrs (sys == system) {
          default = images;
          inherit images;
          inherit (images)
            kernel
            dtb
            tools
            initramfs
            rootfs
            boot
            recovery
            tossd
            ;
        }
      );

      nixosModules = rec {
        toss-connector = import ./nix/nixos-module.nix {
          packageFor =
            sys:
            self.packages.${sys}.toss-connector
              or (throw "toss-connector: ${sys}용 패키지가 없습니다(지원: ${toString hostSystems}). programs.toss-connector.package로 직접 지정해 주세요");
        };
        default = toss-connector;
      };
      overlays.default = final: _prev: { toss-connector = self.packages.${final.stdenv.hostPlatform.system}.toss-connector; };

      checks.${system}.toss-connector-module = import ./nix/module-check.nix {
        inherit
          self
          nixpkgs
          pkgs
          system
          ;
      };

      devShells.${system} = {
        default = pkgs.mkShell {
          packages = with pkgs; [
            (python3.withPackages (ps: [ ps.pillow ]))
            rkdeveloptool
            android-tools
            usbutils
            picocom
            e2fsprogs
            xz
            dtc
            llvmPackages.clang-unwrapped
            llvmPackages.lld
            llvmPackages.llvm
            gnumake
            bison
            flex
            bc
            perl
            openssl
            elfutils
            pkg-config
            rsync
            openssh
            tinyproxy
            fakeroot
            apk-tools
            curl
            file
            cpio
            lz4
            xxd
            rust
          ];
        };

        re = pkgs.mkShell {
          packages = with pkgs; [
            ghidra
            jadx
            llvmPackages.llvm
            python3
            binutils
          ];
        };
      };
    };
}
