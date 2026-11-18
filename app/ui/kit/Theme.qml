// 전역 디자인 토큰. 색상, 폰트, 애니메이션 시간, 여러 컴포넌트가 공유하는 치수 및 상수를 정의한다
// (특정 컴포넌트 내부에서만 쓰는 치수는 해당 파일에 둔다).
//
// 용어(`docs/03-userland-guide.md` 3.16절)
//   타일      위젯을 배치하는 그리드 칸. 페이지마다 4x6으로 구성된다
//   페이지    타일 그리드 1판. 좌우 스와이프로 전환한다
//   앱        기능 묶음(예: Media). 앱마다 1종 이상의 위젯을 제공한다
//   위젯 종류 앱이 제공하는 위젯 타입(예: `media.player`). 점유 타일 크기가 정해져 있다
//   위젯      위젯 종류의 인스턴스. 개별 설정을 가진다
//     버튼 위젯          랙 1칸에 들어가는 소형 위젯(버튼, 스위치 버튼, 노브). 반드시 랙 내부에 배치된다
//     복합 위젯          단일 버튼으로 표현할 수 없는 위젯. 위젯 프레임 안에 아래 컴포넌트를 조합해 구성한다.
//                        랙과 동등하게 페이지에 직접 놓이며, 내부에 다른 위젯을 담을 수 없다
//   랙        버튼 위젯을 담는 컨테이너(그룹/폴더 역할)
//   컴포넌트  버튼(토글 상태가 있으면 스위치 버튼) / 노브 / 디스플레이 / 인디케이터
//
// 디자인 규칙 — "실제 하드웨어 장비의 물리 원리"만 차용하고, 과도한 질감이나 실사 묘사는 배제한다.
//   1. 조명은 위에서 비춘다. 파인 홈은 상단 모서리에 그림자가 지고 하단 모서리가 밝게 빛난다.
//   2. 컴포넌트는 앞판(`plate`)에 뚫린 홈에 끼워져 있다. 버튼과 디스플레이는 동일한 형태의 홈 테두리를 공유한다.
//        앞판(plate)         화면 배경. 타일이 배치되는 베이스 패널
//        위젯 틀(frame)      랙과 복합 위젯의 외곽 프레임. 높이(입체감)는 두지 않고 옅은 면과 가는 선으로 영역만 구분한다.
//                            네 모서리의 작은 나사로 패널에 고정된 모듈임을 표현한다
//        버튼(button)        홈에서 돌출되어 있으며, 누르면 홈 안쪽으로 들어간다. 누를 수 있는 모든 요소는 버튼으로 만든다
//        노브(knob)          원형 로터리 컨트롤. 버튼과 같은 홈에서 같은 높이로 돌출되며, 윗면 포인터가 값을 가리킨다. 눈금은 앞판에 인쇄된다
//        디스플레이(display) 유리는 앞판과 같은 평면이고, 콘텐츠는 유리 아래 들어간 면에 배치한다(깊이감은 그림자로만 표현).
//                            텍스트, 리스트, 이미지처럼 동적으로 바뀌는 정보는 모두 디스플레이 안에 담는다
//      돌출되는 것은 버튼(과 노브)뿐이고, 들어간 것은 디스플레이 내부 콘텐츠뿐이다. 위젯 프레임 자체에는 높이를 두지 않는다.
//   3. 상태는 인디케이터로 표시한다. 꺼짐 / 켜짐 / 점멸(진행 중).
//   4. 모서리는 직각에 가깝게 처리한다. 실제 장비의 패널과 버튼을 기준으로 삼는다(화면 외곽의 둥근 모서리는 기기 베젤에 맞추기 위한 예외).
//   5. 색상은 절제한다. 패널 전체는 무채색으로 두고, 색상은 몇 가지 버튼 컬러와 인디케이터 상태색에만 사용한다.
//
// 라이트 모드와 다크 모드 두 벌을 제공한다. `dark`를 전환하면 모든 색상이 `k`(0=라이트, 1=다크)를 따라 부드럽게 보간된다.
pragma Singleton
import QtQuick

QtObject {
    id: t

    property bool dark: false
    property bool glass: true
    property real k: dark ? 1 : 0
    Behavior on k { NumberAnimation { duration: t.tPage; easing.type: Easing.InOutQuad } }

    function mix(a, b, f) {
        return Qt.rgba(a.r + (b.r - a.r) * f, a.g + (b.g - a.g) * f, a.b + (b.b - a.b) * f, a.a + (b.a - a.a) * f)
    }
    function pick(light, dark) { return mix(Qt.color(light), Qt.color(dark), k) }

    readonly property color plate: pick("#d9d7d1", "#16181b")
    readonly property color plateShade: pick("#cdcbc4", "#101214")
    readonly property color frame: pick("#e6e4de", "#212429")
    readonly property color edgeHi: pick("#fbfaf7", "#3b3f46")
    readonly property color edgeLo: pick("#b2afa7", "#0a0b0d")
    readonly property color well: pick("#9f9c94", "#040506")
    readonly property color screwHi: pick("#f6f5f1", "#5a5e66")
    readonly property color screwLo: pick("#b9b6ae", "#24272b")
    readonly property color screwSlot: pick("#80000000", "#b0000000")
    readonly property color shadow: pick("#42000000", "#a0000000")
    readonly property string inkOnLight: "#26282b"
    readonly property string inkOnDark: "#f4f5f6"
    readonly property color ink: pick(inkOnLight, "#e7e9eb")
    readonly property color inkDim: pick("#74767a", "#868b92")

    readonly property color display: pick("#d2d5cb", "#07090a")
    readonly property color displayInk: pick("#1f231f", "#edf1ee")
    readonly property color displayDim: pick("#667064", "#7f8b87")
    readonly property color displayOff: pick("#bdc1b6", "#262b2e")
    readonly property color displayShade: pick("#5a000000", "#b0000000")
    readonly property color displaySeam: pick("#a3a199", "#000000")
    readonly property color displayGlare: pick("#2affffff", "#13ffffff")
    readonly property color accent: "#ff5f1f"

    readonly property color indicatorOff: pick("#aeaba4", "#3a3e44")
    readonly property color indicatorSeat: pick("#9a978f", "#0c0d0f")
    readonly property color indicatorGreen: "#2fe07a"
    readonly property color indicatorAmber: "#ffad1f"
    readonly property color indicatorRed: "#ff4433"
    readonly property color indicatorBlue: "#3d9dff"

    readonly property var buttonColors: ({
        bone: ["#f2f0ea", "#484c53"],
        graphite: ["#474a4f", "#2c2f34"],
        orange: ["#ff6a2a", "#ee5e22"],
        yellow: ["#f7c843", "#e0b233"],
        blue: ["#3f82e6", "#3974cc"],
        red: ["#e84c3d", "#d34335"],
        green: ["#3fb26c", "#379f60"]
    })
    function buttonColor(name) {
        const c = buttonColors[name] || buttonColors.bone
        return pick(c[0], c[1])
    }
    function inkOn(c) {
        return 0.299 * c.r + 0.587 * c.g + 0.114 * c.b > 0.58 ? Qt.color(inkOnLight) : Qt.color(inkOnDark)
    }
    function indicatorFor(name) {
        return name === "red" ? indicatorRed : name === "orange" ? indicatorAmber : indicatorGreen
    }

    readonly property int screenInset: 2
    readonly property int screenRadius: 28
    readonly property int gridMargin: 24
    readonly property int gridMarginTop: 27
    readonly property int gridMarginV: 10
    readonly property int gridGap: 12
    readonly property int rFrame: 3
    readonly property int rButton: 3
    readonly property int rDisplay: 2
    readonly property int rDetail: 1
    readonly property int buttonTravel: 5
    readonly property int displayShadow: 14
    readonly property int screwSize: 10
    readonly property int screwInset: 6
    readonly property int rackButtonW: 132
    readonly property int rackButtonH: 80
    readonly property int framePad: 12
    readonly property int labelLine: 32
    readonly property int barInner: 48
    readonly property int barHeight: gridMarginTop + barInner + 4
    readonly property real dimmed: 0.45
    readonly property int labelMax: 24

    readonly property string font: "Noto Sans CJK KR"
    readonly property string mono: "Noto Sans Mono CJK KR"
    readonly property int fLabel: 13
    readonly property int fSmall: 16
    readonly property int fBody: 20
    readonly property int fTitle: 24

    readonly property int tPress: 60
    readonly property int tRelease: 190
    readonly property int tState: 150
    readonly property int tToggle: 70
    readonly property int tPanel: 220
    readonly property int tPage: 260
    readonly property int tHold: 1500
}
