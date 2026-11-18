// 배치 편집 드래그 상태 관리자: 위젯/랙 이동(`move`)과 랙 크기 조절(`resize`) 상태 및 충돌 판정을 담당한다(시각 요소 없음).
// 터치 입력은 `shell/Placed`의 편집 오버레이가 받고, 드롭 가이드와 드래그 프리뷰 렌더링은 `shell/Pager`가 담당하며,
// 두 컴포넌트 모두 이 상태를 참조한다.
//
// `grid`는 `Pager` 인스턴스다. 읽는 값: `cols`, `rows`, `pages`, `editing`, `tileW`, `tileH`, `tileX()`, `tileY()`, `width`, `currentIndex`, `count`.
// 호출하는 메서드/시그널: 페이지 전환 `show(page)`, 드래그 완료 `moved(target, id, page, x, y)` / `resized(id, w, h)`.
//
//   mode       `""`(대기) / `"move"` / `"resize"`
//   target, info, page   드래그 대상: `"widget"` 또는 `"rack"`, 배치 데이터 객체, 원래 속해 있던 페이지 인덱스
//   item       드래그 중인 원본 아이템 참조(이동 시에만 사용)
//   proxyX/Y   드래그 프리뷰 위치(`grid` 기준 좌표)
//   dropX/Y/W/H, dropOk   드롭될 타일 좌표 및 배치 가능 여부(그리드 범위 내이며 기존 요소와 겹치지 않는지.
//                         리사이즈 시에는 랙 내부 버튼 위젯이 모두 수용되어야 함).
//   settling   손을 뗀 뒤 데몬의 새 배치 응답을 기다리는 짧은 대기 상태(이 동안 프리뷰를 드롭 위치에 유지).
// 호출 순서: `beginMove -> moveTo ... -> end`, `beginResize -> resizeTo ... -> end`. `finish()`는 즉시 드래그 상태를 종료한다
// (새 배치가 도착했거나 편집 모드를 종료할 때).
// 이동 중 포인터가 화면 좌우 가장자리(`edgeBand`)에 머물면 인접 페이지로 자동 전환된다.
// `slop`은 드래그 시작 임계 거리다: 이 거리 이상 움직여야 드래그로 인식해 오버레이 버튼 탭과 구분한다.
import QtQuick
import "../kit"

Item {
    id: drag
    property var grid: null

    readonly property int slop: 8
    readonly property int edgeBand: 36
    property string mode: ""
    property string target: ""
    property var info: ({})
    property int page: 0
    property Item item: null
    property point offset: Qt.point(0, 0)
    property real proxyX: 0
    property real proxyY: 0
    property int dropX: 0
    property int dropY: 0
    property int dropW: 1
    property int dropH: 1
    property bool dropOk: false
    property bool settling: false

    function fits(page, x, y, w, h) {
        if (x < 0 || y < 0 || x + w > grid.cols || y + h > grid.rows)
            return false
        const there = grid.pages[page]
        if (!there)
            return page === grid.pages.length && grid.editing
        const hit = (i) => x < i.x + i.w && i.x < x + w && y < i.y + i.h && i.y < y + h
        for (const r of there.racks)
            if (!(drag.target === "rack" && r.id === drag.info.id) && hit(r))
                return false
        for (const i of there.widgets)
            if (!(drag.target === "widget" && i.id === drag.info.id) && hit(i))
                return false
        return true
    }
    function beginMove(target, info, page, item, at, offset) {
        finish()
        drag.target = target
        drag.info = info
        drag.page = page
        drag.item = item
        drag.offset = offset
        drag.mode = "move"
        moveTo(at)
    }
    function moveTo(at) {
        proxyX = at.x - offset.x
        proxyY = at.y - offset.y
        dropW = info.w
        dropH = info.h
        dropX = Math.max(0, Math.min(grid.cols - dropW, Math.round((proxyX - Theme.gridMargin) / (grid.tileW + Theme.gridGap))))
        dropY = Math.max(0, Math.min(grid.rows - dropH, Math.round((proxyY - Theme.gridMarginV) / (grid.tileH + Theme.gridGap))))
        dropOk = fits(grid.currentIndex, dropX, dropY, dropW, dropH)
        edge.direction = at.x < edgeBand ? -1 : at.x > grid.width - edgeBand ? 1 : 0
    }
    function beginResize(info, page) {
        finish()
        drag.target = "rack"
        drag.info = info
        drag.page = page
        drag.item = null
        dropX = info.x
        dropY = info.y
        dropW = info.w
        dropH = info.h
        dropOk = true
        drag.mode = "resize"
    }
    function resizeTo(at) {
        dropW = Math.max(1, Math.min(grid.cols - info.x, Math.round((at.x - grid.tileX(info.x) + Theme.gridGap) / (grid.tileW + Theme.gridGap))))
        dropH = Math.max(1, Math.min(grid.rows - info.y, Math.round((at.y - grid.tileY(info.y) + Theme.gridGap) / (grid.tileH + Theme.gridGap))))
        let inside = true
        for (const w of info.widgets || [])
            if (w.x + w.w > dropW || w.y + w.h > dropH)
                inside = false
        dropOk = inside && fits(page, info.x, info.y, dropW, dropH)
    }
    function end() {
        edge.direction = 0
        if (mode === "move" && dropOk && (grid.currentIndex !== page || dropX !== info.x || dropY !== info.y)) {
            grid.moved(target, info.id, grid.currentIndex, dropX, dropY)
            proxyX = grid.tileX(dropX)
            proxyY = grid.tileY(dropY)
            settleSoon()
        } else if (mode === "resize" && dropOk && (dropW !== info.w || dropH !== info.h)) {
            grid.resized(info.id, dropW, dropH)
            settleSoon()
        } else {
            finish()
        }
    }
    function settleSoon() {
        settling = true
        settle.restart()
    }
    function finish() {
        settle.stop()
        settling = false
        mode = ""
        item = null
    }

    Timer {
        id: settle
        interval: 500
        onTriggered: drag.finish()
    }
    Timer {
        id: edge
        property int direction: 0
        interval: 650
        repeat: true
        running: direction !== 0 && drag.mode === "move" && !drag.settling
        onTriggered: {
            const next = drag.grid.currentIndex + direction
            if (next >= 0 && next < drag.grid.count) {
                drag.grid.show(next)
                drag.dropOk = drag.fits(next, drag.dropX, drag.dropY, drag.dropW, drag.dropH)
            }
        }
    }
}
