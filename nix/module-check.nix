# NixOS 모듈(`nix/nixos-module.nix`) 검증 테스트. `flake.nix`에서 `checks.<시스템>.toss-connector-module`로 노출한다.
#
#   nix flake check        (또는 nix build .#checks.x86_64-linux.toss-connector-module)
#
# 몇 가지 설정 조합으로 NixOS 구성을 평가하고 생성된 결과를 검증한다.
#   - 기본 활성화: 그래픽 세션에 바인딩된 사용자 서비스가 생성되고 바이너리 및 PATH가 올바른지 확인(`managed.json` 미생성).
#     시스템 사용자(`@system`)에서는 실행되지 않으며, 인자 오류(2)나 중복 실행(3) 종료 시 자동 재시작하지 않도록 설정된다.
#   - `extraArgs` 지정: systemd 확장(`%`, `$`) 없이 인자가 원문 그대로 전달되도록 기록되는지 확인.
#   - `autostart = false`: 서비스는 생성되지 않고 패키지만 설치되는지 확인.
#   - `config` 지정: `/etc/toss-connector/managed.json`이 선언대로 생성되고(ID는 이름 기반 생성)
#     알 수 없는 권한은 `toss-connector`가 거절하는지 확인.
#
# `self`는 현재 flake, `nixpkgs`는 nixpkgs 입력, `pkgs`와 `system`은 체크를 실행할 호스트 시스템이다.
{
  self,
  nixpkgs,
  pkgs,
  system,
}:
let
  connector = self.packages.${system}.toss-connector;
  minimal = {
    boot.loader.grub.enable = false;
    fileSystems."/" = {
      device = "none";
      fsType = "tmpfs";
    };
    system.stateVersion = "25.05";
  };
  eval =
    module:
    nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        self.nixosModules.default
        minimal
        module
      ];
    };
  nixos = eval {
    programs.toss-connector.enable = true;
    programs.toss-connector.path = [ pkgs.hello ];
  };
  off = eval {
    programs.toss-connector.enable = true;
    programs.toss-connector.autostart = false;
  };
  unit = nixos.config.systemd.user.units."toss-connector.service";
  withArgs =
    (eval {
      programs.toss-connector.enable = true;
      programs.toss-connector.console.enable = false;
      programs.toss-connector.extraArgs = [
        "--config"
        "/tmp/50%/$x y"
      ];
    }).config.systemd.user.units."toss-connector.service";
  withConfig =
    cfg:
    (eval {
      programs.toss-connector.enable = true;
      programs.toss-connector.config = cfg;
    }).config.environment.etc."toss-connector/managed.json".source;
  managed = withConfig {
    permissions = [
      "media.info"
      "command.run"
    ];
    commands = [
      {
        name = "Terminal";
        run = "foot";
      }
      {
        name = "Mode";
        id = "0000beef";
        kind = "stepped";
        steps = [
          "eco"
          {
            name = "Game";
            run = "game-mode on";
          }
        ];
        set = "mode {}";
        state = "mode get";
      }
      {
        name = "Volume";
        kind = "linear";
        max = 100;
        step = 5;
        set = "vol {}%";
        state = "vol get";
        every = 1;
      }
    ];
  };
  broken = withConfig { permissions = [ "everything" ]; };
in
assert !(nixos.config.environment.etc ? "toss-connector/managed.json");
assert !(off.config.systemd.user.units ? "toss-connector.service");
assert builtins.any (p: "${p}" == "${connector}") off.config.environment.systemPackages;
assert unit.wantedBy == [ "graphical-session.target" ];
pkgs.runCommand "toss-connector-module-check"
  {
    text = unit.text;
    argsText = withArgs.text;
    passAsFile = [
      "text"
      "argsText"
    ];
  }
  ''
    cat $textPath
    grep -q 'ExecStart=${connector}/bin/toss-connector$' $textPath
    grep -q 'PartOf=graphical-session.target' $textPath
    grep -q 'After=graphical-session.target' $textPath
    grep -q 'ConditionUser=!@system' $textPath
    grep -q 'RestartPreventExitStatus=2 3' $textPath
    grep -qF 'ExecStart=${connector}/bin/toss-connector "--no-console" "--config" "/tmp/50%%/$$x y"' $argsTextPath
    grep -q 'PATH=${pkgs.hello}/bin:/run/wrappers/bin:.*/etc/profiles/per-user/%u/bin:.*/run/current-system/sw/bin' $textPath

    m=${managed}
    cat $m
    ${pkgs.jq}/bin/jq -e '.strict == true and .permissions == ["media.info", "command.run"] and (.commands | length) == 3' $m
    ${pkgs.jq}/bin/jq -e '(.commands[0].id | test("^[0-9a-f]{8}$")) and .commands[1].id == "0000beef"' $m
    ${pkgs.jq}/bin/jq -e '.commands[1].steps == [{"name": "eco"}, {"name": "Game", "run": "game-mode on"}] and .commands[2].max == 100 and .commands[2].step == 5 and .commands[2].every == 1' $m
    echo '{"permissions":["everything"]}' > bad.json
    if ${connector}/bin/toss-connector managed check bad.json; then
      echo "잘못된 권한 값을 거절하지 않았습니다" >&2
      exit 1
    fi
    : ${builtins.toString (builtins.isString broken.drvPath)}

    cp $textPath $out
  ''
