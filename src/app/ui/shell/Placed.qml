// 페이지에 배치된 단일 요소 래퍼: 랙이나 복합 위젯을 메타데이터(`info`의 `x`, `y`, `w`, `h` 타일 단위)에 맞춰 `shell/Pager` 위에 배치한다.
// 내부 자식(`Rack` 또는 `WidgetSlot`)이 해당 영역을 채우며 좌표와 크기는 `grid`(`Pager`)에서 계산한다.
//
// 편집 모드(`grid.editing`)에서는 내부 위젯 입력을 막고 편집용 오버레이를 띄운다:
//   - 오버레이를 드래그하면 이동한다. 일정 거리(`shell/LayoutDrag`의 `slop`) 이상 움직여야 드래그가 시작되므로 오버레이 버튼을 탭할 때 실수로 들리지 않는다.
//     드래그 중인 원본 위치는 반투명 처리된다(`lifted`).
//   - 모서리의 `x` 버튼을 누르면 삭제 요청을 발생시킨다(`grid.removed(target, info)`).
//   - 랙(`target === "rack"`)인 경우: 우하단 리사이즈 핸들을 드래그해 크기를 변경하고, 라벨 영역을 탭하면 이름 변경을 요청한다(`grid.renameAsked`).
//     각 칸에는 버튼이 하나씩 표시된다: 빈 칸의 `+`는 버튼 추가(`grid.buttonAddAsked`), 채워진 칸의 `x`는 버튼 삭제(`grid.removed("button", widget)`).
// 드래그 상태와 스냅/충돌 판정은 `grid.drag`(`LayoutDrag`)에서 처리하며, 여기서는 포인터 좌표만 전달한다.
import QtQuick
import "../kit"

Item {
    id: placed
    property var grid: null
    property string target: "widget"
    property var info: ({})
    property int pageIndex: 0
    default property alias content: holder.data

    readonly property var drag: grid ? grid.drag : null
    readonly property bool editing: !!grid && grid.editing
    readonly property bool lifted: !!drag && drag.mode === "move" && drag.target === target && drag.info.id === info.id

    x: grid ? grid.tileX(info.x || 0) : 0
    y: grid ? grid.tileY(info.y || 0) : 0
    width: grid ? grid.spanW(info.w || 1) : 0
    height: grid ? grid.spanH(info.h || 1) : 0
    opacity: lifted ? 0.22 : 1

    Item { id: holder; anchors.fill: parent; enabled: !placed.editing }

    Item {
        anchors.fill: parent
        visible: placed.editing
        Rectangle {
            anchors.fill: parent
            radius: Theme.rFrame
            color: "transparent"
            border.width: 2
            border.color: Theme.mix(Theme.plate, Theme.ink, 0.45)
        }
        MouseArea {
            anchors.fill: parent
            preventStealing: true
            property point grip: Qt.point(0, 0)
            property bool moving: false
            function finish() {
                if (!moving)
                    return
                moving = false
                placed.drag.end()
            }
            onPressed: (m) => {
                grip = Qt.point(m.x, m.y)
                moving = false
            }
            onPositionChanged: (m) => {
                const at = mapToItem(placed.grid, m.x, m.y)
                if (moving) {
                    if (placed.drag.mode === "move" && !placed.drag.settling)
                        placed.drag.moveTo(at)
                } else if (Math.abs(m.x - grip.x) >= placed.drag.slop || Math.abs(m.y - grip.y) >= placed.drag.slop) {
                    moving = true
                    placed.drag.beginMove(placed.target, placed.info, placed.pageIndex, holder, at, grip)
                }
            }
            onReleased: finish()
            onCanceled: finish()
        }
        MouseArea {
            visible: placed.target === "rack"
            width: Math.min(parent.width - 60, 240)
            height: 36
            onClicked: placed.grid.renameAsked(placed.info)
        }
        Repeater {
            model: placed.target === "rack" ? (placed.info.w || 0) * (placed.info.h || 0) : 0
            Item {
                id: cell
                required property int index
                readonly property int cx: index % placed.info.w
                readonly property int cy: Math.floor(index / placed.info.w)
                readonly property real topPad: placed.info.label ? Theme.labelLine : Theme.framePad
                readonly property var held: {
                    for (const w of placed.info.widgets || [])
                        if (w.x === cx && w.y === cy)
                            return w
                    return null
                }
                width: (placed.width - 2 * Theme.framePad) / placed.info.w
                height: (placed.height - topPad - Theme.framePad) / placed.info.h
                x: Theme.framePad + cx * width
                y: topPad + cy * height
                Button {
                    id: cellKey
                    anchors.centerIn: parent
                    anchors.verticalCenterOffset: cell.held ? 0 : 6
                    width: 52
                    height: 44
                    onTapped: cell.held ? placed.grid.removed("button", cell.held) : placed.grid.buttonAddAsked(placed.info.id, cell.cx, cell.cy)
                    Glyph { anchors.centerIn: parent; name: cell.held ? "close" : "plus"; size: 20; lineWidth: 2.6; color: cellKey.inkColor }
                }
            }
        }
        Button {
            id: removeKey
            anchors { right: parent.right; top: parent.top; margins: 5 }
            width: 44
            height: 40
            onTapped: placed.grid.removed(placed.target, placed.info)
            Glyph { anchors.centerIn: parent; name: "close"; size: 20; lineWidth: 2.6; color: removeKey.inkColor }
        }
        Item {
            visible: placed.target === "rack"
            anchors { right: parent.right; bottom: parent.bottom }
            width: 48
            height: 48
            Glyph {
                anchors { right: parent.right; bottom: parent.bottom; margins: 7 }
                name: "grip"
                size: 22
                lineWidth: 2.4
                color: Theme.ink
            }
            MouseArea {
                anchors.fill: parent
                preventStealing: true
                onPressed: placed.drag.beginResize(placed.info, placed.pageIndex)
                onPositionChanged: (m) => { if (placed.drag.mode === "resize" && !placed.drag.settling) placed.drag.resizeTo(mapToItem(placed.grid, m.x, m.y)) }
                onReleased: placed.drag.end()
                onCanceled: placed.drag.end()
            }
        }
    }
}
