// 웹페이지 위젯 공통 베이스: `web` 앱의 2개 위젯(`web.button`, `web.icon`)이 공유하는 속성과 동작을 제공한다. 시각 요소는 없다.
//
// 대상 PC와 주소는 위젯 설정(`widget.settings.host`, `widget.settings.url`)에서 가져온다. 누르면 그 PC의 기본 브라우저에서 주소를 연다
// (`web.open` 액션. PC는 http/https 주소만 연다).
//   ready   PC가 연결되어 있고 그 PC가 이 덱에 `web.open` 권한을 허용했는지 여부. `false`면 흐리게(`fade`) 비활성화된다.
//   face, label  버튼 색상과 라벨(위젯 설정 기준)
//   open()  주소 열기 요청
import QtQuick
import "../kit"

WidgetBase {
    id: root

    readonly property string url: widget.settings && widget.settings.url ? widget.settings.url : ""
    readonly property bool ready: !!host && url !== "" && !!host.caps && host.caps.indexOf("web.open") >= 0
    readonly property color face: Theme.buttonColor(widget.color || "bone")
    readonly property string label: widget.label || ""
    readonly property real fade: ready ? 1 : Theme.dimmed

    function open() {
        if (ready)
            action("web.open", { url: url })
    }
}
