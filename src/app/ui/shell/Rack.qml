// 랙: 버튼 위젯(버튼, 스위치, 노브)을 담는 컨테이너(그룹/폴더 역할). 라벨이 있는 위젯 프레임 내부를 랙의 타일 수(`w x h`)에 맞춰
// 균등 분할하고, 각 셀에 버튼 위젯을 하나씩 배치한다(`shell/WidgetSlot`).
// 버튼 위젯은 반드시 랙 내부에 놓이고, 복합 위젯은 랙에 들어가지 않고 랙과 동등하게 페이지에 직접 배치된다
// (이 규칙은 데몬의 `src/app/tossd/src/layout/mod.rs` `check`가 검증한다).
// `rack`은 데몬이 내려 준 랙 배치 객체이고, `link`는 하위 `WidgetSlot`으로 그대로 전달한다.
import QtQuick
import "../kit"

Item {
    id: root
    property var rack: ({})
    property var link: null

    WidgetFrame {
        anchors.fill: parent
        label: root.rack.label || ""

        Repeater {
            model: root.rack.widgets
            WidgetSlot {
                required property var modelData
                readonly property real cellW: parent ? parent.width / (root.rack.w || 1) : 0
                readonly property real cellH: parent ? parent.height / (root.rack.h || 1) : 0
                x: modelData.x * cellW
                y: modelData.y * cellH
                width: modelData.w * cellW
                height: modelData.h * cellH
                widget: modelData
                link: root.link
            }
        }
    }
}
