// 웹페이지 아이콘 버튼 위젯(`web` 앱의 `web.icon`): `WebButtonWidget`과 같되 버튼 윗면에 사이트 아이콘을 그린다. 랙 내부에 배치된다.
// 아이콘은 덱이 가져와 보관한 것을 쓴다(`icons`: 주소 -> `data:` URI. 데몬의 `icons.rs`). 아직 가져오지 못했으면 지구본 모양을 그린다.
import QtQuick
import "../kit"

WebBase {
    id: root
    property var icons: ({})

    LabeledButton {
        anchors.fill: parent
        opacity: root.fade
        color: root.face
        label: root.label
        indicator: false
        image: root.icons[root.url] || ""
        glyph: "globe"
        onTapped: root.open()
    }
}
