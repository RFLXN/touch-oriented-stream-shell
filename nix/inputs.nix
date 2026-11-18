# 이미지 빌드 입력 중 git 저장소 외부에서 오는 두 가지 아티팩트를 해시로 고정해 제공한다. `nix/build.nix`에서 참조한다.
#
#   Alpine 패키지(`os/alpine/lock.json`에서 파일명과 sha256 고정)
#     lock, repoUrl   락파일 내용과 저장소 이름(`main`, `community`)을 미러 URL로 변환하는 함수
#     minirootfs      Alpine 기본 rootfs 타르볼
#     apkRepo         고정된 패키지만 포함한 로컬 APK 저장소(rootfs 빌드 시 여기서만 설치)
#     apkOf           이름으로 특정 패키지 파일 검색(initramfs에서 `busybox-static` 등을 추출할 때 사용)
#     declared        설치할 패키지 목록(`os/alpine/packages`)
#   순정 펌웨어 추출물(`os/stock.json`에 sha256만 기록)
#     firmware        무선 펌웨어를 `/lib/firmware` 구조로 배치한 디렉터리
#     stockBoot       순정 부트 이미지(복구용 커널 추출 원본)
#
# 순정 펌웨어 추출물은 제조사 소유 바이너리이므로 저장소에 포함하지 않는다. Nix 스토어에 없으면 빌드를 중단하고
# `scripts/stock-to-store.sh` 실행을 안내한다. Alpine 패키지는 미러에서 받으며, 미러에서 내려갔으면
# `scripts/alpine-lock.sh restore`로 로컬 아카이브(`archive/alpine`)에서 복구한다.
{ pkgs }:
let
  inherit (pkgs) lib;

  lock = lib.importJSON ../os/alpine/lock.json;
  repoUrl = repo: "${lock.mirror}/${lock.branch}/${repo}";

  apks = map (
    p:
    pkgs.fetchurl {
      url = "${repoUrl p.repo}/aarch64/${p.file}";
      inherit (p) sha256;
    }
  ) lock.packages;

  apkOf =
    name:
    lib.findFirst (
      a: builtins.match "${lib.escapeRegex name}-[0-9][^-]*-r[0-9]+\\.apk" a.name != null
    ) (throw "lock.json에 ${name} 패키지가 없습니다") apks;

  minirootfs = pkgs.fetchurl {
    url = "${lock.mirror}/${lock.branch}/releases/aarch64/${lock.minirootfs.file}";
    inherit (lock.minirootfs) sha256;
  };

  apkRepo = pkgs.runCommand "toss-front-apk-repo" { nativeBuildInputs = [ pkgs.apk-tools ]; } ''
    mkdir -p $out/aarch64
    ${lib.concatMapStringsSep "\n" (a: "ln -s ${a} $out/aarch64/${a.name}") apks}
    apk index --allow-untrusted --rewrite-arch aarch64 -o $out/aarch64/APKINDEX.tar.gz $out/aarch64/*.apk
  '';

  declared = lib.pipe (lib.fileContents ../os/alpine/packages) [
    (lib.splitString "\n")
    (map (l: lib.head (lib.splitString "#" l)))
    (map lib.trim)
    (lib.filter (l: l != ""))
  ];

  stock = lib.importJSON ../os/stock.json;
  stockMessage = ''
    순정 펌웨어 추출 파일이 Nix 스토어에 없습니다. 다음 명령을 실행하세요:
      nix develop -c scripts/stock-to-store.sh
    (필요 파일: downgrade/image/ 아래 순정 761 이미지)
  '';
  stockFile =
    name: sha256:
    pkgs.runCommandLocal name
      {
        outputHash = sha256;
        outputHashAlgo = "sha256";
        outputHashMode = "flat";
      }
      ''
        echo ${lib.escapeShellArg stockMessage} >&2
        exit 1
      '';
  firmware = pkgs.runCommand "toss-front-firmware" { } (
    lib.concatStrings (
      lib.mapAttrsToList (path: sha256: ''
        install -D -m 644 ${stockFile (baseNameOf path) sha256} $out/${path}
      '') stock.firmware
    )
  );
  stockBoot = stockFile stock.boot.name stock.boot.sha256;
in
{
  inherit
    lock
    repoUrl
    apkOf
    minirootfs
    apkRepo
    declared
    firmware
    stockBoot
    ;
}
