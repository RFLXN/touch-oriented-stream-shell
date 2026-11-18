// 페이지 컨테이너. 좌우 스와이프로 전환한다. 페이지마다 `cols x rows` 타일 그리드가 있고 그 위에 랙과 복합 위젯을 배치한다(`shell/Placed`).
// 타일 크기는 여기서 계산하며 랙과 위젯의 모든 치수는 이를 기준으로 결정된다(`tileX`/`tileY`는 좌표, `spanW`/`spanH`는 점유 크기).
// 데이터는 `link`(`shell/DaemonLink`)에서 읽는다: `pages`, `cols`, `rows`는 여기서 사용하고
// 호스트, 값, 메트릭 데이터는 랙과 위젯 슬롯으로 그대로 넘긴다.
//
// 편집 모드(`editing`): 배치를 편집한다. 변경 요청만 시그널로 전달하며 실제 반영은 데몬이 처리하고,
// 데몬이 새 `pages`를 내려 주면 그에 맞춰 다시 그린다.
//   - 기존 위젯과 랙 입력이 비활성화되고 그리드 셀 가이드가 연하게 표시된다.
//   - 위젯/랙을 드래그하면 이동한다(`moved`). 타일에 스냅되며 빈자리에만 배치된다(가능은 초록, 불가는 빨강 하이라이트).
//     화면 좌우 가장자리로 끌고 잠시 유지하면 인접 페이지로 넘어가며, 마지막에는 빈 페이지가 하나 추가되어 새 페이지로 이동할 수 있다.
//   - 모서리의 `x`를 누르면 삭제 요청을 보낸다(`removed(target, info)` — 랙이면 내부 위젯 정보도 포함).
//   - 랙은 우하단 핸들을 드래그해 크기를 조절하고(`resized`), 라벨 영역을 탭하면 이름을 변경한다(`renameAsked`).
//   - 랙의 빈 칸에는 `+` 버튼이 표시되어 버튼 위젯 추가 메뉴를 열 수 있고(`buttonAddAsked`),
//     채워진 칸의 `x` 버튼을 누르면 해당 버튼 위젯 삭제 요청을 보낸다(`removed("button", ...)`).
// 드래그 상태와 판정 로직은 `drag`(`shell/LayoutDrag`)에 있으며, 여기서는 그 상태를 보고 드롭 프리뷰를 그린다.
// 편집 중에도 빈 영역을 스와이프하면 페이지를 넘길 수 있다.
import QtQuick
import "../kit"

Item {
    id: pager
    property var link: null
    property bool editing: false
    signal buttonAddAsked(int rack, int x, int y)
    signal moved(string target, int id, int page, int x, int y)
    signal resized(int id, int w, int h)
    signal removed(string target, var info)
    signal renameAsked(var rack)

    readonly property var pages: link ? link.pages : []
    readonly property int cols: link ? link.cols : 4
    readonly property int rows: link ? link.rows : 6
    readonly property alias currentIndex: view.currentIndex
    readonly property alias count: view.count
    readonly property alias drag: layoutDrag
    readonly property int maxPages: 12
    readonly property int pageCount: pages.length + (editing && pages.length < maxPages ? 1 : 0)
    readonly property var emptyPage: ({ name: "", racks: [], widgets: [] })

    function show(index) {
        view.currentIndex = Math.max(0, Math.min(index, view.count - 1))
    }

    readonly property real tileW: Math.floor((width - 2 * Theme.gridMargin - (cols - 1) * Theme.gridGap) / cols)
    readonly property real tileH: Math.floor((height - 2 * Theme.gridMarginV - (rows - 1) * Theme.gridGap) / rows)
    function tileX(x) { return Theme.gridMargin + x * (tileW + Theme.gridGap) }
    function tileY(y) { return Theme.gridMarginV + y * (tileH + Theme.gridGap) }
    function spanW(w) { return w * tileW + (w - 1) * Theme.gridGap }
    function spanH(h) { return h * tileH + (h - 1) * Theme.gridGap }

    property int keptIndex: 0
    property bool rebuilding: false
    function rebuild() {
        rebuilding = true
        view.model = pageCount
        view.currentIndex = Math.max(0, Math.min(keptIndex, pageCount - 1))
        view.positionViewAtIndex(view.currentIndex, ListView.Beginning)
        rebuilding = false
        keptIndex = view.currentIndex
    }
    onPageCountChanged: rebuild()
    Component.onCompleted: rebuild()
    onPagesChanged: layoutDrag.finish()
    onEditingChanged: if (!editing) layoutDrag.finish()

    LayoutDrag {
        id: layoutDrag
        grid: pager
    }

    ListView {
        id: view
        anchors.fill: parent
        orientation: ListView.Horizontal
        snapMode: ListView.SnapOneItem
        highlightRangeMode: ListView.StrictlyEnforceRange
        highlightMoveDuration: Theme.tPage
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        interactive: layoutDrag.mode === ""
        cacheBuffer: Math.max(0, pager.width * pager.maxPages)
        onCurrentIndexChanged: if (!pager.rebuilding) pager.keptIndex = currentIndex

        delegate: Item {
            id: page
            required property int index
            readonly property var info: pager.pages[index] || pager.emptyPage
            width: pager.width
            height: pager.height

            Repeater {
                model: pager.editing ? pager.cols * pager.rows : 0
                Rectangle {
                    required property int index
                    x: pager.tileX(index % pager.cols)
                    y: pager.tileY(Math.floor(index / pager.cols))
                    width: pager.tileW
                    height: pager.tileH
                    radius: Theme.rFrame
                    color: "transparent"
                    border.width: 1
                    border.color: Theme.mix(Theme.plate, Theme.edgeLo, 0.5)
                }
            }
            Repeater {
                model: page.info.racks.length
                Placed {
                    id: rackPlace
                    required property int index
                    grid: pager
                    target: "rack"
                    info: page.info.racks[index] || ({})
                    pageIndex: page.index
                    Rack {
                        anchors.fill: parent
                        rack: rackPlace.info
                        link: pager.link
                    }
                }
            }
            Repeater {
                model: page.info.widgets.length
                Placed {
                    id: widgetPlace
                    required property int index
                    grid: pager
                    target: "widget"
                    info: page.info.widgets[index] || ({})
                    pageIndex: page.index
                    WidgetSlot {
                        anchors.fill: parent
                        widget: widgetPlace.info
                        link: pager.link
                    }
                }
            }
        }
    }

    Rectangle {
        visible: layoutDrag.mode !== "" && !layoutDrag.settling
        x: pager.tileX(layoutDrag.dropX)
        y: pager.tileY(layoutDrag.dropY)
        width: pager.spanW(layoutDrag.dropW)
        height: pager.spanH(layoutDrag.dropH)
        radius: Theme.rFrame
        color: Qt.alpha(border.color, 0.12)
        border.width: 3
        border.color: layoutDrag.dropOk ? Theme.indicatorGreen : Theme.indicatorRed
    }
    Item {
        visible: layoutDrag.mode === "move" && layoutDrag.item !== null
        x: layoutDrag.proxyX
        y: layoutDrag.proxyY
        width: pager.spanW(layoutDrag.info.w || 1)
        height: pager.spanH(layoutDrag.info.h || 1)
        ShaderEffectSource {
            anchors.fill: parent
            sourceItem: layoutDrag.item
            live: false
            onSourceItemChanged: scheduleUpdate()
            opacity: 0.92
        }
        Rectangle {
            anchors.fill: parent
            radius: Theme.rFrame
            color: "transparent"
            border.width: 2
            border.color: Theme.ink
        }
    }
}
