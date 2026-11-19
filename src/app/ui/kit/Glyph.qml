// 벡터 아이콘 컴포넌트. 버튼 위(미디어 제어, 설정)나 `IconIndicator`의 아이콘으로 사용한다.
// 현재는 필요한 아이콘만 SVG 패스로 직접 내장해 두었다(아이콘 세트가 정해지면 교체 예정).
//
// 24x24 좌표계로 정의한 패스를 `size`에 맞춰 스케일링한다. 아이콘마다 채움 패스(`fills`)와 외곽선 패스(`lines`)를 따로 둘 수 있다.
//   play pause prev next   미디어 재생 제어
//   rew ffwd               탐색(뒤로 / 앞으로, 겹친 삼각형 2개)
//   repeat repeat1 shuffle 반복(되돌아오는 화살표 2개), 한 곡 반복(중앙 1), 셔플(엇갈리는 화살표 2개)
//   gear                   설정
//   usb lan wlan bt core   네트워크/연결 인터페이스(`core`는 데몬 연결용 칩 아이콘)
//   chevron back lock      목록 탐색(`>`), 뒤로가기(`<`), 잠금
//   close plus grip        배치 편집: 삭제(`x`), 추가(`+`), 크기 조절 핸들(빗금 2개)
//   shift backspace enter hide globe   가상 키보드 기능 키
//   heart hearton queue device         Spotify: 좋아요(빈 하트 / 채운 하트), 대기열, 재생 기기
import QtQuick
import QtQuick.Shapes

Shape {
    id: root
    property string name: "play"
    property color color: Theme.ink
    property real size: 24
    property real lineWidth: 2
    property real grow: 0

    readonly property var fills: ({
        play: "M8 5 L19 12 L8 19 Z",
        pause: "M7 5 H10.5 V19 H7 Z M13.5 5 H17 V19 H13.5 Z",
        prev: "M6 5 H8.5 V19 H6 Z M19 5 L9.5 12 L19 19 Z",
        next: "M15.5 5 H18 V19 H15.5 Z M5 5 L14.5 12 L5 19 Z",
        rew: "M12 6 L3.5 12 L12 18 Z M20.5 6 L12 12 L20.5 18 Z",
        ffwd: "M3.5 6 L12 12 L3.5 18 Z M12 6 L20.5 12 L12 18 Z",
        gear: "M10.17 5.14 L10.38 2.54 L13.62 2.54 L13.83 5.14 L15.56 5.86 L17.54 4.16 L19.84 6.46 L18.14 8.44 L18.86 10.17 L21.46 10.38 L21.46 13.62 L18.86 13.83 L18.14 15.56 L19.84 17.54 L17.54 19.84 L15.56 18.14 L13.83 18.86 L13.62 21.46 L10.38 21.46 L10.17 18.86 L8.44 18.14 L6.46 19.84 L4.16 17.54 L5.86 15.56 L5.14 13.83 L2.54 13.62 L2.54 10.38 L5.14 10.17 L5.86 8.44 L4.16 6.46 L6.46 4.16 L8.44 5.86 Z M8.7 12 A3.3 3.3 0 1 0 15.3 12 A3.3 3.3 0 1 0 8.7 12 Z",
        usb: "M12 1.6 L14.6 5.4 H9.4 Z M10.3 20.2 A1.7 1.7 0 1 0 13.7 20.2 A1.7 1.7 0 1 0 10.3 20.2 Z M4.2 10.4 A1.4 1.4 0 1 0 7 10.4 A1.4 1.4 0 1 0 4.2 10.4 Z M17.2 6.2 H19.8 V8.8 H17.2 Z",
        lan: "M9.5 3.5 H14.5 V7.5 H9.5 Z M3.5 16.5 H8.5 V20.5 H3.5 Z M15.5 16.5 H20.5 V20.5 H15.5 Z",
        wlan: "M10.8 18 A1.2 1.2 0 1 0 13.2 18 A1.2 1.2 0 1 0 10.8 18 Z",
        core: "M10.6 10.6 H13.4 V13.4 H10.6 Z",
        lock: "M6.5 11.5 H17.5 V20 H6.5 Z",
        hearton: "M12 20 L4.3 12.4 A4.4 4.4 0 0 1 12 6.9 A4.4 4.4 0 0 1 19.7 12.4 Z",
        queue: "M15 14 L20.5 17.25 L15 20.5 Z"
    })
    readonly property var lines: ({
        usb: "M12 18.5 V5 M12 17 L5.6 13.6 V12 M12 14 L18.5 10.6 V9.4",
        lan: "M12 8 V12 M6 16 V12 H18 V16",
        wlan: "M8.46 14.46 A5 5 0 0 1 15.54 14.46 M5.64 11.64 A9 9 0 0 1 18.36 11.64 M2.81 8.81 A13 13 0 0 1 21.19 8.81",
        bt: "M7 7.5 L17 16.5 L12 21 V3 L17 7.5 L7 16.5",
        core: "M7.5 7.5 H16.5 V16.5 H7.5 Z M10 4 V7.5 M14 4 V7.5 M10 16.5 V20 M14 16.5 V20 M4 10 H7.5 M4 14 H7.5 M16.5 10 H20 M16.5 14 H20",
        repeat: "M4 11.5 V9.5 A2.5 2.5 0 0 1 6.5 7 H19 M16 4 L19 7 L16 10 M20 12.5 V14.5 A2.5 2.5 0 0 1 17.5 17 H5 M8 14 L5 17 L8 20",
        repeat1: "M4 10 V8 A2.5 2.5 0 0 1 6.5 5.5 H19 M16 2.5 L19 5.5 L16 8.5 M20 14 V16 A2.5 2.5 0 0 1 17.5 18.5 H5 M8 15.5 L5 18.5 L8 21.5 M10.6 10.6 L12.5 9.3 V14.9",
        shuffle: "M3 7 H6.5 L14.5 17 H20 M17 14 L20 17 L17 20 M3 17 H6.5 L9 13.9 M12 10.1 L14.5 7 H20 M17 4 L20 7 L17 10",
        close: "M6.5 6.5 L17.5 17.5 M17.5 6.5 L6.5 17.5",
        plus: "M12 5 V19 M5 12 H19",
        grip: "M20 9 L9 20 M20 15 L15 20",
        chevron: "M9 5 L16 12 L9 19",
        back: "M15 5 L8 12 L15 19",
        lock: "M8.5 11 V8 A3.5 3.5 0 0 1 15.5 8 V11",
        shift: "M12 4 L4.5 12 H8.5 V19 H15.5 V12 H19.5 Z",
        backspace: "M9 6 H20.5 V18 H9 L3.5 12 Z M12.5 9.5 L17 14.5 M17 9.5 L12.5 14.5",
        enter: "M19 6 V13.5 H6 M10 9.5 L6 13.5 L10 17.5",
        hide: "M3.5 5 H20.5 V14 H3.5 Z M9 17.5 L12 20.5 L15 17.5 M8 11 H16",
        globe: "M3.5 12 A8.5 8.5 0 1 0 20.5 12 A8.5 8.5 0 1 0 3.5 12 Z M3.5 12 H20.5 M12 3.5 C7.5 7.5 7.5 16.5 12 20.5 M12 3.5 C16.5 7.5 16.5 16.5 12 20.5",
        heart: "M12 20 L4.3 12.4 A4.4 4.4 0 0 1 12 6.9 A4.4 4.4 0 0 1 19.7 12.4 Z",
        queue: "M3.5 6 H19.5 M3.5 11 H19.5 M3.5 16 H11",
        device: "M3.5 5 H20.5 V15.5 H3.5 Z M8.5 19.5 H15.5 M12 15.5 V19.5"
    })

    width: size
    height: size
    preferredRendererType: Shape.CurveRenderer

    ShapePath {
        fillColor: root.color
        fillRule: ShapePath.OddEvenFill
        strokeColor: root.color
        strokeWidth: 1.5 + 2 * root.grow
        joinStyle: ShapePath.RoundJoin
        scale: Qt.size(root.size / 24, root.size / 24)
        PathSvg { path: root.fills[root.name] || "" }
    }
    ShapePath {
        fillColor: "transparent"
        strokeColor: root.color
        strokeWidth: root.lineWidth * root.size / 24 + 2 * root.grow
        capStyle: ShapePath.RoundCap
        joinStyle: ShapePath.RoundJoin
        scale: Qt.size(root.size / 24, root.size / 24)
        PathSvg { path: root.lines[root.name] || "" }
    }
}
