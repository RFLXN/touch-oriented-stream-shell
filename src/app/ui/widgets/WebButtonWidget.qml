// 웹페이지 버튼 위젯(`web` 앱의 `web.button`): 누르면 대상 PC의 기본 브라우저에서 설정한 주소를 연다. 랙 내부에 배치된다.
// 명령 버튼(`CommandWidget`)과 같은 모양이다(상단 버튼 + 하단 라벨, 인디케이터 없음). 동작과 비활성 조건은 `widgets/WebBase` 참고.
import QtQuick
import "../kit"

WebBase {
    id: root

    LabeledButton {
        anchors.fill: parent
        opacity: root.fade
        color: root.face
        label: root.label
        indicator: false
        onTapped: root.open()
    }
}
