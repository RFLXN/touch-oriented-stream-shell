# 기기 플래싱용 이미지를 빌드하는 `buildImage` 함수를 제공한다.
#
# 사용 예(`flake.nix`에서 호출):
#   inherit (import ./nix/build.nix { inherit pkgs rust; }) buildImage;
#   images = buildImage { hostname = "toss-front"; ssh.lan = true; };
# `rust`는 기기 타깃(`aarch64-unknown-linux-musl`)으로 크로스 컴파일 가능한 cargo/rustc 툴체인이다(`tossd` 빌드에 사용).
# 옵션 기본값은 파일 하단(`sshDefaults` 등 및 `buildImage` 정의)에, 각 옵션 설명은 `docs/02-progress.md` 2.6절에 있다.
# 출력 디렉터리에는 이미지 3종이 담기며, 개별 파생물(`kernel`, `dtb`, `initramfs`, `rootfs`, `boot`, `recovery`, `tools`, `tossd`)을 `passthru` 속성으로 노출한다.
#
# 빌드 입력은 네 가지로 나뉜다.
#   1. 이 저장소의 파일(`src/os/`, `src/app/`, `src/scripts/` 일부)   — git으로 추적
#   2. nixpkgs가 고정하는 소스/도구(커널 소스, 빌드 툴)    — `flake.lock`
#   3. Alpine 패키지(버전 및 해시 고정)                   — `src/os/alpine/lock.json`
#   4. 순정 펌웨어 추출물(무선 펌웨어, 순정 커널)           — `src/os/stock.json`에 해시만 기록
# 4번은 제조사 소유 바이너리이므로 저장소에 넣지 않고 `src/scripts/stock-to-store.sh`로 Nix 스토어에 등록한다.
#
# 구성 파일은 세 개로 나뉜다.
#   nix/inputs.nix   3번과 4번 입력(Alpine 패키지, 순정 펌웨어 추출물)
#   nix/kernel.nix   커널 및 디바이스 트리
#   이 파일          기타: 기기용 바이너리 도구, initramfs, 데몬, rootfs, 부트 이미지 2종, 옵션 및 `buildImage`
# 메인라인 커널 커맨드라인은 `src/os/kernel/cmdline`에, 복구용(순정 커널) 커맨드라인은 이 파일의 `recoveryCmdline`에 정의되어 있다.
#
# git이 추적하는 작업 트리의 파일(커밋하지 않은 수정 포함)이 같으면 동일한 이미지가 생성된다(타임스탬프/UUID/파일 정렬 순서 고정).
{ pkgs, rust }:
let
  inherit (pkgs) lib;
  fs = lib.fileset;
  only = fileset: fs.toSource { root = ../.; inherit fileset; };

  inherit (import ./inputs.nix { inherit pkgs; })
    lock
    repoUrl
    apkOf
    minirootfs
    apkRepo
    declared
    firmware
    stockBoot
    ;
  inherit (import ./kernel.nix { inherit pkgs only; }) kernel dtb;

  tools =
    pkgs.runCommand "toss-front-tools"
      {
        nativeBuildInputs = with pkgs; [
          llvmPackages.clang-unwrapped
          llvmPackages.lld
        ];
        src = only ../src/os/tools;
      }
      ''
        mkdir -p $out/bin
        for t in reboot-mode i2c-read; do
          clang --target=aarch64-unknown-linux-gnu -static -nostdlib -ffreestanding -fno-stack-protector \
            -O2 -fuse-ld=lld -o $out/bin/$t $src/src/os/tools/$t.c
        done
      '';

  initramfs =
    pkgs.runCommand "toss-front-initramfs"
      {
        nativeBuildInputs = with pkgs; [
          fakeroot
          cpio
        ];
        src = only (
          fs.unions [
            ../src/os/initramfs
            ./build-initramfs.sh
          ]
        );
        busyboxApk = apkOf "busybox-static";
        regdbApk = apkOf "wireless-regdb";
        inherit firmware;
      }
      ''
        mkdir -p $out
        fakeroot -- bash $src/nix/build-initramfs.sh
      '';

  rustPlatform = pkgs.makeRustPlatform {
    cargo = rust;
    rustc = rust;
  };
  tossd = pkgs.stdenv.mkDerivation {
    pname = "tossd";
    inherit ((lib.importTOML ../src/app/tossd/Cargo.toml).package) version;
    src = only (
      fs.unions [
        ../src/app/tossd
        ../src/app/proto
        ../src/app/meter
      ]
    );
    sourceRoot = "source/src/app/tossd";
    cargoDeps = rustPlatform.importCargoLock { lockFile = ../src/app/tossd/Cargo.lock; };
    nativeBuildInputs = [
      rust
      rustPlatform.cargoSetupHook
    ];
    buildPhase = ''
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
      cargo build --release --offline --target aarch64-unknown-linux-musl
    '';
    doCheck = true;
    checkPhase = ''
      cargo test --offline
    '';
    installPhase = ''
      install -D -m 755 target/aarch64-unknown-linux-musl/release/tossd $out/bin/tossd
    '';
    dontFixup = true;
  };
  tossFiles = only (
    fs.unions [
      ../src/app/ui
      ../src/app/rootfs
    ]
  );

  mkRootfs =
    {
      hostname,
      ssh,
      ntp,
      kiosk,
      toss,
    }:
    pkgs.runCommand "toss-front-rootfs"
      {
        nativeBuildInputs = with pkgs; [
          fakeroot
          apk-tools
          e2fsprogs
          python3
        ];
        src = only (
          fs.unions [
            ../src/os/rootfs
            ../src/scripts/gen-beep.py
            ./build-rootfs.sh
          ]
        );
        inherit
          minirootfs
          apkRepo
          firmware
          tools
          kernel
          ;
        packages = lib.concatStringsSep " " declared;
        repositories = lib.concatMapStringsSep "\n" repoUrl [
          "main"
          "community"
        ];
        inherit hostname;
        sshLan = lib.boolToString ssh.lan;
        authorizedKeys = lib.concatLines ssh.authorizedKeys;
        ntpEnable = lib.boolToString ntp.enable;
        inherit (ntp) timezone;
        kioskEnable = lib.boolToString kiosk.enable;
        kioskCommand = kiosk.command;
        tossEnable = lib.boolToString toss.enable;
        tossd = lib.optionalString toss.enable "${tossd}";
        tossFiles = lib.optionalString toss.enable "${tossFiles}";
        zoneinfo = "${pkgs.tzdata}/share/zoneinfo/${ntp.timezone}";
      }
      ''
        mkdir -p $out
        fakeroot -- bash $src/nix/build-rootfs.sh
      '';

  cmdline = lib.pipe (lib.fileContents ../src/os/kernel/cmdline) [
    (lib.splitString "\n")
    (map lib.trim)
    (lib.filter (l: l != "" && !lib.hasPrefix "#" l))
    (lib.concatStringsSep " ")
  ];
  recoveryCmdline = "console=ttyFIQ0 rootwait panic=5 loglevel=7";
  bootTools = {
    nativeBuildInputs = with pkgs; [
      android-tools
      python3
    ];
    src = only (
      fs.unions [
        ../src/scripts/mkboot.sh
        ../src/scripts/rsce.py
      ]
    );
  };

  boot = pkgs.runCommand "toss-front-boot" bootTools ''
    mkdir -p $out
    bash $src/src/scripts/mkboot.sh -p boot \
      -k ${kernel}/Image -r ${initramfs}/initramfs.cpio.gz -d ${dtb}/rk3399-toss-front.dtb \
      -c ${lib.escapeShellArg cmdline} -o $out/boot-mainline.img
  '';

  recovery = pkgs.runCommand "toss-front-recovery" bootTools ''
    mkdir -p $out
    unpack_bootimg --boot_img ${stockBoot} --out stock >/dev/null
    bash $src/src/scripts/mkboot.sh -p recovery \
      -k stock/kernel -r ${initramfs}/initramfs.cpio.gz -d stock/dtb -s stock/second \
      -c ${lib.escapeShellArg recoveryCmdline} -o $out/boot-stage1.img
  '';

  sshDefaults = {
    lan = false;
    authorizedKeys = [ ];
  };

  ntpDefaults = {
    enable = false;
    timezone = "Asia/Seoul";
  };

  kioskDefaults = {
    enable = false;
    command = "foot";
  };

  tossDefaults = {
    enable = false;
  };

  withDefaults =
    name: defaults: given:
    let
      unknown = lib.attrNames (removeAttrs given (lib.attrNames defaults));
    in
    assert lib.assertMsg (unknown == [ ]) "buildImage: ${name}에 알 수 없는 옵션이 있습니다: ${toString unknown}";
    defaults // given;

  buildImage =
    {
      hostname ? "toss-front",
      ssh ? { },
      ntp ? { },
      kiosk ? { },
      toss ? { },
    }:
    let
      rootfs = mkRootfs {
        inherit hostname;
        ssh = withDefaults "ssh" sshDefaults ssh;
        ntp = withDefaults "ntp" ntpDefaults ntp;
        kiosk = withDefaults "kiosk" kioskDefaults kiosk;
        toss = withDefaults "toss" tossDefaults toss;
      };
    in
    pkgs.runCommand "toss-front-images"
      {
        passthru = {
          inherit
            kernel
            dtb
            tools
            initramfs
            rootfs
            boot
            recovery
            tossd
            ;
        };
      }
      ''
        mkdir -p $out
        ln -s ${boot}/boot-mainline.img $out/boot-mainline.img
        ln -s ${recovery}/boot-stage1.img $out/boot-stage1.img
        ln -s ${rootfs}/alpine-rootfs.img $out/alpine-rootfs.img
        ln -s ${rootfs}/packages.txt $out/packages.txt
        {
          echo "kernel   $(cat ${kernel}/kernel.release)"
          echo "cmdline  "${lib.escapeShellArg cmdline}
          echo "alpine   ${lock.minirootfs.file}, 패키지 $(wc -l <${rootfs}/packages.txt)개"
          cd $out && sha256sum boot-mainline.img boot-stage1.img alpine-rootfs.img
        } >$out/manifest.txt
        printf '%s\n' ${kernel} ${dtb} ${initramfs} ${tools} ${firmware} ${apkRepo} ${minirootfs} ${stockBoot} >$out/inputs.txt
      '';
in
{
  inherit buildImage;
}
