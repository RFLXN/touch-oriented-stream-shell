# `toss-connector`: 덱과 통신하는 PC 측 데몬/CLI(`app/host`). 덱에 연결해 상태를 전송하고 요청된 액션을 실행하며 웹 콘솔을 제공한다.
# 호스트 PC용으로 빌드하며 기기 이미지에는 포함되지 않는다.
#
#   nix build .#toss-connector  ->  result/bin/toss-connector
#
# `flake.nix`가 시스템별로 이 파일을 호출해 `packages.<시스템>.toss-connector`로 노출하고, NixOS 모듈(`nix/nixos-module.nix`)이 이를 사용한다.
# `rust`는 cargo와 rustc가 포함된 툴체인이다(`flake.nix`에서 `rust-overlay`로 제공하며 버전은 `flake.lock`으로 고정).
{
  lib,
  stdenv,
  makeRustPlatform,
  rust,
}:
let
  fs = lib.fileset;
  rustPlatform = makeRustPlatform {
    cargo = rust;
    rustc = rust;
  };
in
stdenv.mkDerivation {
  pname = "toss-connector";
  inherit ((lib.importTOML ../app/host/Cargo.toml).package) version;
  src = fs.toSource {
    root = ../.;
    fileset = fs.unions [
      ../app/host
      ../app/proto
      ../app/meter
    ];
  };
  sourceRoot = "source/app/host";
  cargoDeps = rustPlatform.importCargoLock { lockFile = ../app/host/Cargo.lock; };
  nativeBuildInputs = [
    rust
    rustPlatform.cargoSetupHook
  ];
  buildPhase = ''
    runHook preBuild
    cargo build --release --offline
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    export HOME=$TMPDIR
    cargo test --release --offline
    runHook postCheck
  '';
  installPhase = ''
    runHook preInstall
    install -D -m 755 target/release/toss-connector $out/bin/toss-connector
    runHook postInstall
  '';
  meta = {
    description = "토스 프론트 스트림덱용 PC 연동 데몬 및 CLI";
    mainProgram = "toss-connector";
    platforms = lib.platforms.linux;
  };
}
