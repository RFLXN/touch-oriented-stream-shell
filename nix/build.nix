# 기기 플래싱용 이미지를 빌드하는 `buildImage` 함수를 제공한다.
#
# 사용 예(`flake.nix`에서 호출):
#   inherit (import ./nix/build.nix { inherit pkgs rust; }) buildImage;
#   images = buildImage { hostname = "toss-front"; ssh.lan = true; };
# `rust`는 기기 타깃(`aarch64-unknown-linux-musl`)으로 크로스 컴파일 가능한 cargo/rustc 툴체인이다(`tossd` 빌드에 사용).
# `tossd`의 TLS(`ring`)는 C 코드를 포함하므로 기기 타깃용 C 컴파일러로 clang을 쓴다(libc 헤더 없이 빌드된다).
# 옵션 기본값은 파일 하단(`sshDefaults` 등 및 `buildImage` 정의)에, 각 옵션 설명은 `docs/02-progress.md` 2.6절에 있다.
# 출력 디렉터리에는 이미지 3종이 담기며, 개별 파생물(`kernel`, `dtb`, `initramfs`, `rootfs`, `boot`, `recovery`, `tools`, `tossd`)을 `passthru` 속성으로 노출한다.
# `boot-inputs.json`에는 부트 이미지를 다시 묶는 데 필요한 입력(커널, DTB, initramfs 경로, 커맨드라인)과 스플래시 설정이 담긴다.
# 부팅 스플래시의 그림은 저작권 때문에 저장소에 두지 않는다. 이 빌드는 그림 없이 부트 이미지를 만들고, 그림(`local/splash/`)은
# `src/scripts/make-boot.sh`가 빌드 뒤에 initramfs에 덧붙여 `dist/boot.img`로 다시 묶는다. 이 빌드는 재생기 `splash-play`만 넣는다.
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
#   이 파일          기타: 기기용 바이너리 도구, 부팅 스플래시 재생기, initramfs, 데몬, rootfs, 부트 이미지 2종, 옵션 및 `buildImage`
# 메인라인 커널 커맨드라인은 `src/os/kernel/cmdline`에, 복구용(순정 커널) 커맨드라인은 이 파일의 `recoveryCmdline`에 정의되어 있다.
# 부팅 스플래시를 켜면 메인라인 커맨드라인에서 `console=tty0`을 빼고 `logo.nologo vt.global_cursor_default=0 tf.splash`를 더한다(`mkCmdline`).
# 복구용 부트 이미지는 재생기 없는 initramfs를 쓴다.
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

  screen = {
    width = 800;
    height = 1280;
  };

  splashPlay =
    pkgs.runCommand "toss-front-splash-play"
      {
        nativeBuildInputs = with pkgs; [
          llvmPackages.clang-unwrapped
          llvmPackages.lld
        ];
        src = only ../src/os/tools/splash-play.c;
      }
      ''
        mkdir -p $out/bin
        clang --target=aarch64-unknown-linux-gnu -static -nostdlib -ffreestanding -fno-stack-protector \
          -O2 -fuse-ld=lld -o $out/bin/splash-play $src/src/os/tools/splash-play.c
      '';

  mkInitramfs =
    splash:
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
        play = lib.optionalString splash "${splashPlay}/bin/splash-play";
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
      pkgs.llvmPackages.clang-unwrapped
      pkgs.llvmPackages.bintools-unwrapped
    ];
    buildPhase = ''
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
      export CC_aarch64_unknown_linux_musl=clang AR_aarch64_unknown_linux_musl=llvm-ar
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

  baseCmdline = lib.pipe (lib.fileContents ../src/os/kernel/cmdline) [
    (lib.splitString "\n")
    (map lib.trim)
    (lib.filter (l: l != "" && !lib.hasPrefix "#" l))
  ];
  mkCmdline =
    splash:
    lib.concatStringsSep " " (
      if splash.enable then
        lib.filter (a: a != "console=tty0") baseCmdline
        ++ [
          "logo.nologo"
          "vt.global_cursor_default=0"
          "tf.splash"
        ]
      else
        baseCmdline
    );
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

  mkBoot =
    initramfs: cmdline:
    pkgs.runCommand "toss-front-boot" bootTools ''
      mkdir -p $out
      bash $src/src/scripts/mkboot.sh -p boot \
        -k ${kernel}/Image -r ${initramfs}/initramfs.cpio.gz -d ${dtb}/rk3399-toss-front.dtb \
        -c ${lib.escapeShellArg cmdline} -o $out/boot-mainline.img
    '';

  recoveryInitramfs = mkInitramfs false;
  recovery = pkgs.runCommand "toss-front-recovery" bootTools ''
    mkdir -p $out
    unpack_bootimg --boot_img ${stockBoot} --out stock >/dev/null
    bash $src/src/scripts/mkboot.sh -p recovery \
      -k stock/kernel -r ${recoveryInitramfs}/initramfs.cpio.gz -d stock/dtb -s stock/second \
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

  splashDefaults = {
    enable = false;
    background = "#000000";
    width = 0;
    align = "center";
    offset = 0;
  };

  checkSplash =
    splash:
    let
      fail = msg: throw "buildImage: splash.${msg}";
    in
    if !splash.enable then
      splash
    else if builtins.match "#[0-9a-fA-F]{6}" splash.background == null then
      fail "background는 \"#rrggbb\" 형식이어야 합니다: ${toString splash.background}"
    else if !(builtins.elem splash.align [ "center" "top" "bottom" ]) then
      fail "align은 \"center\", \"top\", \"bottom\" 중 하나여야 합니다: ${toString splash.align}"
    else if !(builtins.isInt splash.width) || splash.width < 0 then
      fail "width는 0 이상의 정수여야 합니다(0이면 원본 크기)"
    else if !(builtins.isInt splash.offset) then
      fail "offset은 정수여야 합니다"
    else
      splash;

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
      splash ? { },
    }:
    let
      splashCfg = checkSplash (withDefaults "splash" splashDefaults splash);
      initramfs = mkInitramfs splashCfg.enable;
      cmdline = mkCmdline splashCfg;
      boot = mkBoot initramfs cmdline;
      bootInputs = pkgs.writeText "boot-inputs.json" (
        builtins.toJSON {
          kernel = "${kernel}/Image";
          dtb = "${dtb}/rk3399-toss-front.dtb";
          initramfs = "${initramfs}/initramfs.cpio.gz";
          inherit cmdline;
          splash =
            if splashCfg.enable then
              {
                inherit (splashCfg)
                  background
                  width
                  align
                  offset
                  ;
                screen = [
                  screen.width
                  screen.height
                ];
              }
            else
              null;
        }
      );
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
        ln -s ${bootInputs} $out/boot-inputs.json
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
