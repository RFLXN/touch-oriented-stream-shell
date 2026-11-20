# 메인라인 커널과 디바이스 트리를 빌드한다. `nix/build.nix`에서 호출한다.
#
#   kernel   `Image`, 설정(`config`), 내장 모듈 목록(`modules.builtin`), 버전(`kernel.release`)
#   dtb      `rk3399-toss-front.dtb`
#
# 빌드 절차는 `src/os/kernel/kbuild.sh`에 정의되어 있으며, 증분 개발 빌드(`src/scripts/build-kernel.sh`)도 같은 스크립트를 공유한다.
# 커널 빌드 입력은 `nixpkgs` 커널 소스와 `src/os/kernel/`의 `kbuild.sh`, `toss-front.config`, `drivers/`, `patches/`뿐이고,
# 디바이스 트리 입력은 `kbuild.sh`와 `dts/`뿐이므로 다른 파일을 수정해도 불필요하게 재빌드되지 않는다.
# 설정 조각(`toss-front.config`)이 경고 없이 그대로 반영되지 않으면 커널 빌드를 실패 처리한다.
#
# `only`는 저장소에서 필요한 파일만 추려 소스를 구성하는 헬퍼 함수다(`nix/build.nix`에서 전달).
{ pkgs, only }:
let
  inherit (pkgs) lib;
  fs = lib.fileset;

  linux = pkgs.linux_6_18;
  kernelEnv = {
    inherit (linux) src version;
    nativeBuildInputs = with pkgs; [
      llvmPackages.clang-unwrapped
      llvmPackages.lld
      llvmPackages.llvm
      bison
      flex
      bc
      perl
      python3
      pkg-config
    ];
    buildInputs = with pkgs; [
      openssl
      elfutils
    ];
    postPatch = ''
      patchShebangs scripts
    '';
    dontConfigure = true;
    dontFixup = true;
  };
  kbuildInit = kdir: ''
    KSRC=$PWD KOUT=$PWD/../out KDIR=${kdir}/src/os/kernel JOBS=$NIX_BUILD_CORES
    source $KDIR/kbuild.sh
  '';

  kernel = pkgs.stdenv.mkDerivation (
    kernelEnv
    // {
      pname = "toss-front-kernel";
      buildPhase = ''
        ${kbuildInit (
          only (
            fs.unions [
              ../src/os/kernel/kbuild.sh
              ../src/os/kernel/toss-front.config
              ../src/os/kernel/drivers
              ../src/os/kernel/patches
            ]
          )
        )}
        tf_add_drivers
        tf_config || { echo "커널 설정 조각(toss-front.config)이 경고 없이 반영되지 않았습니다"; exit 1; }
        mk Image
      '';
      installPhase = ''
        mkdir -p $out
        cp ../out/arch/arm64/boot/Image $out/Image
        cp ../out/.config $out/config
        cp ../out/modules.builtin $out/modules.builtin
        cp ../out/include/config/kernel.release $out/kernel.release
      '';
    }
  );

  dtb = pkgs.stdenv.mkDerivation (
    kernelEnv
    // {
      pname = "toss-front-dtb";
      buildPhase = ''
        ${kbuildInit (
          only (
            fs.unions [
              ../src/os/kernel/kbuild.sh
              ../src/os/kernel/dts
            ]
          )
        )}
        tf_add_dts
        mk defconfig >/dev/null
        mk rockchip/rk3399-toss-front.dtb
      '';
      installPhase = ''
        install -D -m 644 ../out/arch/arm64/boot/dts/rockchip/rk3399-toss-front.dtb $out/rk3399-toss-front.dtb
      '';
    }
  );
in
{
  inherit kernel dtb;
}
