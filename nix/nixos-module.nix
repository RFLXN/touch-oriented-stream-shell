# NixOS 모듈: `programs.toss-connector` — 덱과 연동되는 PC 측 데몬(`toss-connector`)을 설치하고 로그인 시 자동 실행한다.
#
# 사용 예(시스템 `flake.nix`):
#   inputs.toss-front.url = "git+file:///home/<사용자>/development/toss-front";   # 또는 원격 저장소 URL
#   outputs = { nixpkgs, toss-front, ... }: {
#     nixosConfigurations.<이름> = nixpkgs.lib.nixosSystem {
#       modules = [ toss-front.nixosModules.default ./configuration.nix ];
#     };
#   };
# `configuration.nix`에 추가:
#   programs.toss-connector.enable = true;
#
# 동작 방식
#   - `toss-connector` 패키지를 시스템에 설치한다(터미널에서 `toss-connector pair`, `toss-connector console` 등 사용 가능).
#   - 사용자 서비스(`systemd --user`)로 등록해 그래픽 세션 시작 시 자동 실행한다.
#     **시스템 서비스가 아니라 사용자 서비스다**: 미디어 플레이어(MPRIS)와 등록된 명령이 로그인한 사용자의 세션에 속하기 때문이다.
#     모든 사용자에게 활성화되지만 페어링된 덱이 없는 사용자 세션에서는 조용히 연결 대기만 한다.
#     시스템 사용자(`@system`, 디스플레이 매니저 등)에게는 실행되지 않는다(`ConditionUser=!@system`).
#     비정상 종료 시 3초 후 재시작하며, 인자 오류(종료 코드 2)나 중복 실행(종료 코드 3)인 경우에는 재시작하지 않는다.
#
# 권한 및 명령을 NixOS 설정으로 선언할 수 있다(`programs.toss-connector.config`):
#   programs.toss-connector.config = {
#     strict = true;                                   # 기본값. 여기에 선언한 항목만 사용(CLI 및 웹 콘솔 수정 차단)
#     permissions = [ "media.info" "media.control" "command.run" ];
#     commands = [
#       { name = "Terminal"; run = "foot"; }
#       { name = "Mic"; kind = "switch"; on = "..."; off = "..."; state = "..."; }
#     ];
#   };
# `strict = false`면 선언된 항목만 고정하고(해제 불가 권한, 수정/삭제 불가 명령) 나머지 권한과 명령은 사용자가 추가할 수 있다.
# `config`를 생략(`null`)하면 모든 권한과 명령을 사용자 설정에 맡긴다.
# 선언된 내용은 읽기 전용 `/etc/toss-connector/managed.json`으로 배치되며, 빌드 타임에 `toss-connector`로 검증해
# 알 수 없는 권한이나 잘못된 명령 형식이 있으면 빌드가 실패한다(`src/app/host/src/managed.rs`).
#
# 실행 중 생성되는 상태(키, 페어링, 덱 주소, 콘솔 토큰)는 각 사용자의 `~/.config/toss-connector`에 저장된다
# (`docs/04-host-link.md` 4.9절, 4.10절). `strict` 모드에서도 페어링은 정상 동작한다.
#
# `packageFor`는 시스템 아키텍처를 받아 해당 플랫폼용 `toss-connector` 패키지를 반환하는 함수다(`flake.nix`에서 전달).
{ packageFor }:
{
  config,
  lib,
  pkgs,
  utils,
  ...
}:
let
  cfg = config.programs.toss-connector;

  commandType = lib.types.submodule {
    options = {
      name = lib.mkOption {
        type = lib.types.str;
        description = "명령 표시 이름(덱 UI에서 선택할 때 노출). 1~40자.";
      };
      id = lib.mkOption {
        type = lib.types.nullOr (lib.types.strMatching "[0-9A-Za-z_-]{1,64}");
        default = null;
        example = "585f6b9d";
        description = ''
          명령 ID. 덱 위젯은 이름이 아니라 이 ID로 명령을 참조한다.
          생략하면 이름 해시에서 자동 생성되므로, 이름을 변경할 때 ID가 바뀌어 기존 위젯을 다시 지정해야 한다.
          이름을 바꿔도 위젯 연결을 유지하거나 기존 수동 명령을 그대로 이관하려면 해당 ID를 명시한다
          (`toss-connector command list`로 확인 가능).
        '';
      };
      kind = lib.mkOption {
        type = lib.types.enum [
          "run"
          "switch"
          "stepped"
          "linear"
        ];
        default = "run";
        description = ''
          명령 종류. run: 단발 실행(버튼), switch: 켜기/끄기(스위치 버튼),
          stepped: 단계 선택(단계 노브), linear: 범위 안의 값 조절(연속 노브).
        '';
      };
      run = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = "run: 실행할 셸 커맨드.";
      };
      on = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = "switch: 켤 때 실행할 커맨드.";
      };
      off = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = "switch: 끌 때 실행할 커맨드.";
      };
      steps = lib.mkOption {
        type = lib.types.listOf (
          lib.types.coercedTo lib.types.str (name: { inherit name; }) (
            lib.types.submodule {
              options = {
                name = lib.mkOption {
                  type = lib.types.str;
                  description = "단계 이름.";
                };
                run = lib.mkOption {
                  type = lib.types.str;
                  default = "";
                  description = "이 단계로 전환할 커맨드. 비우면 상위 명령의 set 템플릿을 사용한다.";
                };
              };
            }
          )
        );
        default = [ ];
        example = lib.literalExpression ''[ "power-saver" "balanced" { name = "Game"; run = "obs-scene game"; } ]'';
        description = "stepped: 단계 목록(2~12개). 문자열만 적으면 set의 `{}` 자리에 해당 이름이 치환된다.";
      };
      set = lib.mkOption {
        type = lib.types.str;
        default = "";
        example = "wpctl set-volume @DEFAULT_SINK@ {}%";
        description = "linear 및 전용 커맨드가 없는 단계에서 값을 적용할 커맨드 템플릿. `{}` 자리에 값이 치환된다.";
      };
      min = lib.mkOption {
        type = lib.types.nullOr lib.types.number;
        default = null;
        description = "linear: 범위의 최솟값(생략 시 0).";
      };
      max = lib.mkOption {
        type = lib.types.nullOr lib.types.number;
        default = null;
        description = "linear: 범위의 최댓값.";
      };
      step = lib.mkOption {
        type = lib.types.nullOr lib.types.number;
        default = null;
        description = "linear: 증감 단위(생략 시 1).";
      };
      state = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = ''
          switch, stepped, linear: 현재 상태를 조회할 커맨드. 비우면 마지막으로 설정한 값만 메모리에서 유지한다.
          switch는 `on`/`off`(또는 종료 코드 0이 ON), stepped는 단계 이름, linear는 숫자 출력을 기대한다.
        '';
      };
      every = lib.mkOption {
        type = lib.types.nullOr lib.types.number;
        default = null;
        description = "상태 폴링 주기(초, 생략 시 2초).";
      };
    };
  };

  toCommand =
    c:
    lib.filterAttrs (_: v: v != null && v != "" && v != [ ]) (
      c
      // {
        id = if c.id != null then c.id else builtins.substring 0 8 (builtins.hashString "sha256" "toss-connector:${c.name}");
        steps = map (s: lib.filterAttrs (_: v: v != "") s) c.steps;
      }
    );

  managedFile =
    pkgs.runCommand "toss-connector-managed.json"
      {
        raw = builtins.toJSON {
          inherit (cfg.config) strict permissions;
          commands = map toCommand cfg.config.commands;
        };
        passAsFile = [ "raw" ];
      }
      ''
        ${lib.getExe cfg.package} managed check "$rawPath" > $out
      '';

  args = lib.optional (!cfg.console.enable) "--no-console" ++ cfg.extraArgs;
in
{
  options.programs.toss-connector = {
    enable = lib.mkEnableOption "toss-connector(토스 프론트 스트림덱용 PC 연동 데몬)";

    package = lib.mkOption {
      type = lib.types.package;
      default = packageFor pkgs.stdenv.hostPlatform.system;
      defaultText = lib.literalExpression "toss-front.packages.\${pkgs.stdenv.hostPlatform.system}.toss-connector";
      description = "사용할 toss-connector 패키지.";
    };

    autostart = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        로그인 시 사용자 서비스로 자동 실행한다. 끄면 패키지만 설치하고 서비스는 생성하지 않는다(터미널에서 `toss-connector` 직접 실행).
      '';
    };

    target = lib.mkOption {
      type = lib.types.str;
      default = "graphical-session.target";
      example = "default.target";
      description = ''
        서비스를 함께 시작할 사용자 systemd 타깃. 기본값은 그래픽 세션 시작 이후로, 등록된 명령이 GUI 창을 띄우려면
        `WAYLAND_DISPLAY` 등 세션 환경 변수가 전달된 뒤에 실행되어야 하기 때문이다.
        GNOME·KDE 등은 이 타깃을 활성화하며, 그렇지 않은 경량 WM 환경에서는 `default.target`으로 변경한다
        (이 경우 WM 시작 시 환경을 systemd에 올려 주어야 GUI 명령이 동작한다:
        `dbus-update-activation-environment --systemd WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP`).
      '';
    };

    console.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        웹 콘솔 활성화 여부. `http://127.0.0.1:7880/`에서만 수신한다. 열 때는 `toss-connector console`을 사용한다.
        끄면 `--no-console`로 실행된다(권한과 명령 관리는 CLI 사용).
      '';
    };

    path = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      example = lib.literalExpression "[ pkgs.wireplumber pkgs.playerctl ]";
      description = ''
        등록된 명령에서 사용할 추가 패키지(서비스 `PATH` 앞부분에 추가).
        별도로 넣지 않아도 시스템·사용자 프로필(`environment.systemPackages`, `users.users.<이름>.packages`,
        home-manager, nix profile)에 설치된 프로그램은 기본 검색 대상에 포함된다.
      '';
    };

    config = lib.mkOption {
      default = null;
      example = lib.literalExpression ''
        {
          strict = false;
          permissions = [ "media.info" "media.control" "command.run" ];
          commands = [
            { name = "Terminal"; run = "foot"; }
            { name = "Volume"; kind = "linear"; max = 100; step = 5; set = "wpctl set-volume @DEFAULT_SINK@ {}%"; }
          ];
        }
      '';
      description = ''
        덱에 허용할 권한과 등록할 명령을 선언적으로 지정한다. `null`이면 고정 설정 없이
        사용자가 `toss-connector` CLI나 웹 콘솔에서 자유롭게 관리한다.
        지정하면 `/etc/toss-connector/managed.json`으로 배포되어 이 PC의 모든 사용자에게 적용된다.
      '';
      type = lib.types.nullOr (
        lib.types.submodule {
          options = {
            strict = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = ''
                true: 선언된 항목만 사용한다. 명시된 권한만 허용되고(나머지는 해제) 명시된 명령만 등록되며,
                CLI나 웹 콘솔에서 권한과 명령을 수정할 수 없다(사용자가 기존에 등록한 명령도 숨겨진다).
                false: 선언된 항목만 고정하고(명시된 권한 해제 불가, 명시된 명령 수정/삭제 불가)
                그 외 권한과 명령은 사용자가 덱별로 추가·관리할 수 있다.
              '';
            };
            permissions = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              example = [
                "media.info"
                "media.control"
                "command.run"
              ];
              description = ''
                허용할 권한 목록(페어링된 모든 덱에 일괄 적용).
                `media.info`(재생 정보 조회), `media.control`(재생 제어), `resource.info`(리소스 조회),
                `command.run`(등록된 명령 실행).
                알 수 없는 권한 이름이 포함되면 빌드가 실패한다.
              '';
            };
            commands = lib.mkOption {
              type = lib.types.listOf commandType;
              default = [ ];
              description = "등록할 명령 목록(덱 버튼/스위치/노브용). 덱에서 실행하려면 `permissions`에 `command.run`을 포함해야 한다.";
            };
          };
        }
      );
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [
        "--config"
        "/path/to/dir"
      ];
      description = "`toss-connector`에 추가로 전달할 인자. systemd 확장(`%`, `$`) 없이 원문 그대로 전달된다.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    environment.etc = lib.mkIf (cfg.config != null) { "toss-connector/managed.json".source = managedFile; };

    systemd.user.services.toss-connector = lib.mkIf cfg.autostart {
      description = "toss-connector: connects this PC to a TOSS deck";
      wantedBy = [ cfg.target ];
      after = [ cfg.target ];
      partOf = [ cfg.target ];
      environment.PATH = lib.mkForce (
        lib.concatStringsSep ":" (
          map (p: "${lib.getBin p}/bin") cfg.path
          ++ [
            "/run/wrappers/bin"
            "%h/.nix-profile/bin"
            "%h/.local/state/nix/profile/bin"
            "/etc/profiles/per-user/%u/bin"
            "/nix/var/nix/profiles/default/bin"
            "/run/current-system/sw/bin"
          ]
        )
      );
      serviceConfig = {
        ExecStart = lib.getExe cfg.package + lib.optionalString (args != [ ]) " ${utils.escapeSystemdExecArgs args}";
        Restart = "on-failure";
        RestartSec = 3;
        RestartPreventExitStatus = "2 3";
      };
      unitConfig.ConditionUser = "!@system";
    };
  };
}
